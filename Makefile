.ONESHELL:
.DEFAULT_GOAL := build

.PHONY: setup build check fix test

setup:
	rustup component add clippy
	cargo install cargo-nextest --locked

build:
	cargo build --workspace

check:
	cargo clippy --no-deps --all -- -Dwarnings -Aunused-variables -Adead-code

fix:
	cargo clippy --fix --allow-dirty --allow-staged --all

test:
	cargo nextest run --workspace
