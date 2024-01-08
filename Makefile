##
# This file is part of the IVMS Online.
#
# @copyright 2024 © by Rafał Wrzeszcz - Wrzasq.pl.
##

SHELL:=bash

default: build

clean:
	cargo clean
	find . -name "*.profraw" -exec rm {} \;
	rm -rf coverage.lcov

build:
	cargo build --release --target x86_64-unknown-linux-gnu

build-dev:
	cargo build --target x86_64-unknown-linux-gnu

package: $(shell find target/x86_64-unknown-linux-gnu/release/ -maxdepth 1 -executable -type f | sed s@x86_64-unknown-linux-gnu/release/\\\(.*\\\)\${$}@\\1.zip@)

test:
	CARGO_INCREMENTAL=0 \
	RUSTFLAGS="-Cinstrument-coverage" \
	LLVM_PROFILE_FILE="cargo-test-%p-%m.profraw" \
	cargo test --all-features --bins

test-local:
	docker run -d --rm --name dynamodb -p 8000:8000 amazon/dynamodb-local:2.2.1
	make test
	docker stop dynamodb

test-integration:
	cargo test --test "*"

check:
	cargo fmt --check
	cargo clippy
	cargo udeps

check-local:
	cargo audit

doc:
	cargo doc --no-deps

fix:
	cargo fmt

lcov:
	grcov . \
		--binary-path ./target/debug/deps/ \
		-s . \
		-t lcov \
		--branch \
		--ignore-not-existing \
		--ignore "../*" \
		--ignore "/*" \
		-o coverage.lcov

coverage:
	grcov . \
		--binary-path ./target/debug/deps/ \
		-s . \
		-t html \
		--branch \
		--ignore-not-existing \
		--ignore "../*" \
		--ignore "/*" \
		-o target/coverage

# generic targets
target/%.zip: target/x86_64-unknown-linux-gnu/release/%
	upx --best $<
	zip -j $@ $^
	printf "@ $(<F)\n@=bootstrap\n" | zipnote -w $@

full: clean build-dev test-local check check-local doc
