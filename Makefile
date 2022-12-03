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
	cargo r -- -c example/conf.yml build -s rpm -- test-patches
	# below should fail
	-cargo r -- -c example/conf.yml build -s rpm -- test-fail-non-existent-patch
	test $? 1
	cargo r -- -c example/conf.yml build -i rocky debian -- test-common-dependencies
	@rpm -qp --requires example/output/rocky/test-common-dependencies-0.1.0-0.x86_64.rpm | grep openssl-devel
	@rpm -qp --conflicts example/output/rocky/test-common-dependencies-0.1.0-0.x86_64.rpm | grep httpd
	@rpm -qp --obsoletes example/output/rocky/test-common-dependencies-0.1.0-0.x86_64.rpm | grep bison1
	@dpkg-deb -I example/output/debian/test-common-dependencies-0.1.0-0.amd64.deb | grep Depends | grep libssl-dev
	@dpkg-deb -I example/output/debian/test-common-dependencies-0.1.0-0.amd64.deb | grep Conflicts | grep apache2


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

