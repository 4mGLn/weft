.PHONY: check docs-check harness-check

check: harness-check docs-check

harness-check:
	./scripts/verify-harness.sh

docs-check:
	python3 scripts/check_docs.py
