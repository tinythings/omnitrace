.DEFAULT_GOAL := build
.PHONY:build

clean:
	cargo clean

check:
	cargo clippy --no-deps --all -- -Dwarnings -Aunused-variables -Adead-code

fix:
	cargo clippy --fix --allow-dirty --allow-staged --all

devel:
	cargo build -v --workspace
	$(call move_bin,debug,)

build:
	cargo build --release --workspace
	$(call move_bin,release,)
