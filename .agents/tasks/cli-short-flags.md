# Task Record: CLI version and verbose short flags

## Outcome

Add conventional short global flags without changing the stable command-result
contract: `-V` aliases `--version`, while `-v` aliases `--verbose`.

## Scope and risk

`--verbose` writes bounded invocation metadata only to standard error. Human
success output and the JSON envelope on standard output must remain unchanged.

## Acceptance evidence

| Check | Result |
| --- | --- |
| `-V` returns the runtime version | Pass (`weft 0.2.0`) |
| `-v` preserves JSON standard output and emits one standard-error diagnostic | Pass (`short_version_and_verbose_flags_preserve_machine_output`) |
| Workspace verification | Pass (`make check`) |
