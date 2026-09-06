# Getting Started with Weft

1. Install the latest supported Linux or macOS release:

   ```bash
   curl -fsSL https://github.com/4mGLn/weft/releases/latest/download/install.sh | sh
   ```

   This bootstrapper comes from the latest published release, not the
   development branch. To select an exact release, use
   `https://github.com/4mGLn/weft/releases/download/vMAJOR.MINOR.PATCH/install.sh`
   and run it with `WEFT_VERSION=vMAJOR.MINOR.PATCH sh`. Set `PREFIX=/path` to
   choose the installation prefix. The script verifies GitHub's asset digest
   before installing.

2. To install a downloaded archive manually:

   ```bash
   tar -xzf weft-<version>-x86_64-unknown-linux-musl.tar.gz
   PREFIX="$HOME/.local" ./weft-<version>-x86_64-unknown-linux-musl/install.sh
   ```

3. Enter the repository where you use agents and wire Weft once:

   ```bash
   cd your-project
   weft setup
   weft doctor
   ```

   Setup creates local durable state, detects supported agent tools, and wires
   their project instruction surfaces where available. It does not launch an
   agent, read a credential, or configure a user-home directory.

4. Continue to use Codex, Claude Code, Gemini CLI, Paseo, or your existing
   orchestrator normally. Weft provides their shared durable coordination state;
   the runtime/orchestrator continues to launch and supervise agents.

5. For an explicit or machine-managed setup, choose runtime names and consume
   the JSON bridge:

   ```bash
   weft --format json setup --runtime codex,claude-code,gemini-cli
   weft --format json doctor
   ```

`USAGE.md` describes the JSON protocol used by agents and orchestrators.
`MANUAL.md` describes local-state and support boundaries.
