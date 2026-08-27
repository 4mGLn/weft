.PHONY: check docs-check harness-check rust-check phase0-spike phase0-native-git-spike phase0-gitbutler-spike phase3-gitbutler-live package-release test-release

RUST_HOST := $(shell rustc -vV | sed -n 's/^host: //p')

check: harness-check docs-check rust-check

harness-check:
	./scripts/verify-harness.sh

docs-check:
	python3 scripts/check_docs.py

rust-check:
	cargo fmt --all --check
	cargo test --workspace --all-targets --target $(RUST_HOST)
	cargo clippy --workspace --all-targets --target $(RUST_HOST) -- -D warnings

phase0-native-git-spike:
	./scripts/phase0-native-git-spike.sh

phase0-gitbutler-spike:
	./scripts/phase0-gitbutler-spike.sh

phase0-spike: phase0-native-git-spike phase0-gitbutler-spike

phase3-gitbutler-live:
	cargo test -p weft-provider-gitbutler --target $(RUST_HOST) tests::live_gitbutler_0_22_stack_export_and_local_landing -- --ignored --exact --nocapture

package-release:
	@test -n "$(VERSION)" || (echo "VERSION is required, e.g. make package-release VERSION=v0.1.0" >&2; exit 2)
	./scripts/package-release.sh "$(VERSION)"

test-release:
	@test -n "$(ARCHIVE)" || (echo "ARCHIVE is required" >&2; exit 2)
	./scripts/test-release.sh "$(ARCHIVE)"
