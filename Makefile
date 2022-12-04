PROJECT := pkger


.PHONY: all
all: clean test build


.PHONY: all_debug
all_debug: clean test build_debug


.PHONY: run_debug
run_debug: build_debug
	@./target/debug/$(PROJECT)


.PHONY: run
run: build
	@./target/release/$(PROJECT)


.PHONY: build_debug
build_debug: ./target/debug/$(PROJECT)


.PHONY: build
build: ./target/release/$(PROJECT)


.PHONY: lint
lint: fmt_check clippy

.PHONY: check
check:
	cargo check --all

.PHONY: test
test:
	cargo t --all-targets --all-features -- --test-threads=1
	cargo r -- -c example/conf.yml build test-package test-suite child-package1 child-package2
	cargo r -- -c example/conf.yml build -s apk -s pkg -- test-package
	# below should fail
	-cargo r -- -c example/conf.yml build -s rpm -- test-fail-non-existent-patch
	test $? 1
	cargo r -- -c example/conf.yml build -s rpm -- test-patches


.PHONY: fmt_check
fmt_check:
	cargo fmt --all -- --check


.PHONY: fmt
fmt:
	cargo fmt --all


.PHONY: clippy
clippy:
	@rustup component add clippy
	cargo clippy --all-targets --all-features -- -D clippy::all


.PHONY: clean
clean:
	@rm -rf target/*


./target/debug/$(PROJECT):
	@cargo build


./target/release/$(PROJECT):
	@cargo build --release

