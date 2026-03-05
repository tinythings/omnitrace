.ONESHELL:
.DEFAULT_GOAL := build

.PHONY:build

check:
	cargo clippy --no-deps --all -- -Dwarnings -Aunused-variables -Adead-code

fix:
	cargo clippy --fix --allow-dirty --allow-staged --all
