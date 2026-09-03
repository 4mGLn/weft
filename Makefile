.PHONY: check docs-check harness-check rust-check phase0-spike phase0-native-git-spike phase0-gitbutler-spike phase5-resume

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

phase5-resume:
	./scripts/test-cli-session-resume.sh
