.ONESHELL:
.DEFAULT_GOAL := build

.PHONY: build check fix test

build:
	cargo build --workspace

check:
	cargo clippy --no-deps --all -- -Dwarnings -Aunused-variables -Adead-code

fix:
	cargo clippy --fix --allow-dirty --allow-staged --all

test:
	cargo nextest run --workspace
