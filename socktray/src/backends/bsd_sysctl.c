#include <arpa/inet.h>
#include <errno.h>
#include <netinet/in.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <sys/sysctl.h>

#if defined(__NetBSD__)
#include <netinet/tcp_fsm.h>
#include <sys/file.h>
#endif

#if defined(__FreeBSD__)
#include <osreldate.h>
#include <libutil.h>
#include <sys/user.h>
#endif

enum {
    SOCKTRAY_PROTO_TCP = 1,
    SOCKTRAY_PROTO_TCP6 = 2,
    SOCKTRAY_PROTO_UDP = 3,
    SOCKTRAY_PROTO_UDP6 = 4,
};

struct socktray_bsd_entry {
    int proto_kind;
    char local[96];
    char remote[96];
    char state[32];
};

struct socktray_vec {
    struct socktray_bsd_entry *data;
    size_t len;
    size_t cap;
};

static int socktray_vec_push(struct socktray_vec *v, const struct socktray_bsd_entry *e) {
    if (v->len == v->cap) {
        size_t next = (v->cap == 0) ? 256 : (v->cap * 2);
        struct socktray_bsd_entry *p =
            (struct socktray_bsd_entry *)realloc(v->data, next * sizeof(*p));
        if (p == NULL) {
            return -1;
        }
        v->data = p;
        v->cap = next;
    }
    v->data[v->len++] = *e;
    return 0;
}

static void socktray_fmt_ep4(
    const struct sockaddr_in *sin,
    char *out,
    size_t out_sz) {
    char ip[INET_ADDRSTRLEN];
    if (sin == NULL || inet_ntop(AF_INET, &sin->sin_addr, ip, sizeof(ip)) == NULL) {
        (void)snprintf(out, out_sz, "0.0.0.0:0");
        return;
    }
    (void)snprintf(out, out_sz, "%s:%u", ip, (unsigned)ntohs(sin->sin_port));
}

static void socktray_fmt_ep6(
    const struct sockaddr_in6 *sin6,
    char *out,
    size_t out_sz) {
    char ip[INET6_ADDRSTRLEN];
    if (sin6 == NULL || inet_ntop(AF_INET6, &sin6->sin6_addr, ip, sizeof(ip)) == NULL) {
        (void)snprintf(out, out_sz, "::0:0");
        return;
    }
    (void)snprintf(out, out_sz, "%s:%u", ip, (unsigned)ntohs(sin6->sin6_port));
}

#if defined(__NetBSD__)
static int socktray_collect_pcblist(
    const char *name,
    int proto_kind,
    int is_tcp,
    struct socktray_vec *outv);

static int socktray_try_collect_pcblist(
    const char *name,
    int proto_kind,
    int is_tcp,
    struct socktray_vec *outv,
    int *any_success) {
    int rc = socktray_collect_pcblist(name, proto_kind, is_tcp, outv);
    if (rc == 0) {
        if (any_success != NULL) {
            *any_success = 1;
        }
        return 0;
    }

    // Some kernels do not expose every family/node (especially inet6).
    // Treat those as optional instead of failing the whole backend.
    if (errno == ENOENT || errno == EINVAL || errno == ENOTSUP || errno == EOPNOTSUPP) {
        return 0;
    }

    return -1;
}

static int socktray_read_pcblist_sysctl(
    const char *name,
    struct kinfo_pcb **out_buf,
    size_t *out_len) {
    int mib_base[16];
    size_t mib_base_len = 16;
    int mode = 0;

    if (out_buf == NULL || out_len == NULL) {
        errno = EINVAL;
        return -1;
    }

    memset(mib_base, 0, sizeof(mib_base));
    if (sysctlnametomib(name, mib_base, &mib_base_len) == -1) {
        return -1;
    }

    // mode 0: plain mib
    // mode 1: mib + (elem_size, elem_count)
    for (mode = 0; mode < 2; mode++) {
        int mib[18];
        size_t miblen = mib_base_len;
        size_t cap = 256 * sizeof(struct kinfo_pcb);
        int tries = 0;

        if (mode == 1) {
            if (miblen + 2 > 18) {
                continue;
            }
            memcpy(mib, mib_base, mib_base_len * sizeof(int));
            mib[miblen++] = (int)sizeof(struct kinfo_pcb);
            mib[miblen++] = (int)(cap / sizeof(struct kinfo_pcb));
        } else {
            memcpy(mib, mib_base, mib_base_len * sizeof(int));
        }

        for (tries = 0; tries < 8; tries++) {
            struct kinfo_pcb *buf = (struct kinfo_pcb *)malloc(cap);
            size_t len = cap;

            if (buf == NULL) {
                errno = ENOMEM;
                return -1;
            }

            if (mode == 1) {
                mib[miblen - 1] = (int)(cap / sizeof(struct kinfo_pcb));
            }

            if (sysctl(mib, (u_int)miblen, buf, &len, NULL, 0) == 0) {
                *out_buf = buf;
                *out_len = len;
                return 0;
            }

            if (errno == ENOMEM || errno == E2BIG) {
                free(buf);
                cap = cap * 2;
                continue;
            }

            free(buf);
            break;
        }
    }

    return -1;
}

static const char *socktray_tcp_state_name(int st) {
    switch (st) {
        case TCPS_ESTABLISHED: return "ESTABLISHED";
        case TCPS_SYN_SENT: return "SYN_SENT";
        case TCPS_SYN_RECEIVED: return "SYN_RECV";
        case TCPS_FIN_WAIT_1: return "FIN_WAIT1";
        case TCPS_FIN_WAIT_2: return "FIN_WAIT2";
        case TCPS_TIME_WAIT: return "TIME_WAIT";
        case TCPS_CLOSED: return "CLOSE";
        case TCPS_CLOSE_WAIT: return "CLOSE_WAIT";
        case TCPS_LAST_ACK: return "LAST_ACK";
        case TCPS_LISTEN: return "LISTEN";
        case TCPS_CLOSING: return "CLOSING";
        default: return "UNKNOWN";
    }
}

static int socktray_collect_pcblist(
    const char *name,
    int proto_kind,
    int is_tcp,
    struct socktray_vec *outv) {
    size_t len = 0;
    struct kinfo_pcb *buf = NULL;
    size_t n = 0;
    size_t i = 0;

    if (socktray_read_pcblist_sysctl(name, &buf, &len) == -1) {
        return -1;
    }
    if (buf == NULL || len == 0) {
        return 0;
    }

    n = len / sizeof(struct kinfo_pcb);
    for (i = 0; i < n; i++) {
        const struct kinfo_pcb *kp = &buf[i];
        struct socktray_bsd_entry e;

        memset(&e, 0, sizeof(e));
        e.proto_kind = proto_kind;

        if (kp->ki_family == AF_INET) {
            socktray_fmt_ep4((const struct sockaddr_in *)&kp->ki_src, e.local, sizeof(e.local));
            socktray_fmt_ep4((const struct sockaddr_in *)&kp->ki_dst, e.remote, sizeof(e.remote));
        } else if (kp->ki_family == AF_INET6) {
            socktray_fmt_ep6((const struct sockaddr_in6 *)&kp->ki_src, e.local, sizeof(e.local));
            socktray_fmt_ep6((const struct sockaddr_in6 *)&kp->ki_dst, e.remote, sizeof(e.remote));
        } else {
            continue;
        }

        if (is_tcp) {
            (void)snprintf(e.state, sizeof(e.state), "%s", socktray_tcp_state_name(kp->ki_tstate));
        } else {
            e.state[0] = '\0';
        }

        if (socktray_vec_push(outv, &e) != 0) {
            free(buf);
            return -1;
        }
    }

    free(buf);
    return 0;
}
#endif

#if defined(__FreeBSD__)
static void socktray_fmt_sa(
    const struct sockaddr *sa,
    char *out,
    size_t out_sz) {
    if (sa == NULL) {
        (void)snprintf(out, out_sz, "0.0.0.0:0");
        return;
    }
    if (sa->sa_family == AF_INET) {
        socktray_fmt_ep4((const struct sockaddr_in *)sa, out, out_sz);
        return;
    }
    if (sa->sa_family == AF_INET6) {
        socktray_fmt_ep6((const struct sockaddr_in6 *)sa, out, out_sz);
        return;
    }
    (void)snprintf(out, out_sz, "0.0.0.0:0");
}

static int socktray_collect_kinfo_file(struct socktray_vec *outv) {
    int cnt = 0;
    int i = 0;
    struct kinfo_file *kif = kinfo_getfile(-1, &cnt);

    if (kif == NULL) {
        return -1;
    }

    for (i = 0; i < cnt; i++) {
        int dom = 0;
        int typ = 0;
        int proto = 0;
        const struct sockaddr *lsa = NULL;
        const struct sockaddr *rsa = NULL;
        struct socktray_bsd_entry e;

#if __FreeBSD_version < 1200031
        if (kif[i].kf_type != KF_TYPE_SOCKET) {
            continue;
        }
        dom = kif[i].kf_sock_domain;
        typ = kif[i].kf_sock_type;
        proto = kif[i].kf_sock_protocol;
        lsa = (const struct sockaddr *)&kif[i].kf_sa_local;
        rsa = (const struct sockaddr *)&kif[i].kf_sa_peer;
#else
        if (kif[i].kf_type != KF_TYPE_SOCKET) {
            continue;
        }
        dom = kif[i].kf_un.kf_sock.kf_sock_domain;
        typ = kif[i].kf_un.kf_sock.kf_sock_type;
        proto = kif[i].kf_un.kf_sock.kf_sock_protocol;
        lsa = (const struct sockaddr *)&kif[i].kf_un.kf_sock.kf_sa_local;
        rsa = (const struct sockaddr *)&kif[i].kf_un.kf_sock.kf_sa_peer;
#endif

        if (typ != SOCK_STREAM && typ != SOCK_DGRAM) {
            continue;
        }
        if (dom != AF_INET && dom != AF_INET6) {
            continue;
        }
        if (proto != IPPROTO_TCP && proto != IPPROTO_UDP) {
            continue;
        }

        memset(&e, 0, sizeof(e));
        if (proto == IPPROTO_TCP) {
            e.proto_kind = (dom == AF_INET6) ? SOCKTRAY_PROTO_TCP6 : SOCKTRAY_PROTO_TCP;
        } else {
            e.proto_kind = (dom == AF_INET6) ? SOCKTRAY_PROTO_UDP6 : SOCKTRAY_PROTO_UDP;
        }

        socktray_fmt_sa(lsa, e.local, sizeof(e.local));
        socktray_fmt_sa(rsa, e.remote, sizeof(e.remote));
        e.state[0] = '\0';

        if (socktray_vec_push(outv, &e) != 0) {
            free(kif);
            return -1;
        }
    }

    free(kif);
    return 0;
}
#endif

int socktray_bsd_collect(struct socktray_bsd_entry **out_entries, size_t *out_count) {
    struct socktray_vec v;
    int rc = 0;

    if (out_entries == NULL || out_count == NULL) {
        errno = EINVAL;
        return -1;
    }

    v.data = NULL;
    v.len = 0;
    v.cap = 0;

#if defined(__NetBSD__)
    int any_success = 0;

    if (socktray_try_collect_pcblist("net.inet.tcp.pcblist", SOCKTRAY_PROTO_TCP, 1, &v, &any_success) != 0) {
        rc = -1;
    }
    if (socktray_try_collect_pcblist("net.inet6.tcp6.pcblist", SOCKTRAY_PROTO_TCP6, 1, &v, &any_success) != 0) {
        rc = -1;
    }
    if (socktray_try_collect_pcblist("net.inet.udp.pcblist", SOCKTRAY_PROTO_UDP, 0, &v, &any_success) != 0) {
        rc = -1;
    }
    if (socktray_try_collect_pcblist("net.inet6.udp6.pcblist", SOCKTRAY_PROTO_UDP6, 0, &v, &any_success) != 0) {
        rc = -1;
    }

    if (rc == 0 && any_success == 0) {
        errno = ENOTSUP;
        rc = -1;
    }
#elif defined(__FreeBSD__)
    rc |= socktray_collect_kinfo_file(&v);
#else
    errno = ENOTSUP;
    return -1;
#endif

    if (rc != 0) {
        free(v.data);
        return -1;
    }

    *out_entries = v.data;
    *out_count = v.len;
    return 0;
}

void socktray_bsd_free(struct socktray_bsd_entry *entries) {
    free(entries);
}
