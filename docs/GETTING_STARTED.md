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

3. Choose a durable local state directory and initialize it:

   ```bash
   weft --state-dir "$HOME/.local/share/weft" init
   ```

4. Create the first durable Change:

   ```bash
   weft --format json --state-dir "$HOME/.local/share/weft" change create \
     --change-id first-change --operation-id first-change-create \
     --actor your-name --at 1000
   ```

5. Inspect it:

   ```bash
   weft --state-dir "$HOME/.local/share/weft" change show \
     --change-id first-change
   ```

Use an application-generated Unix-millisecond timestamp for production
automation rather than the illustrative `1000` above. See `USAGE.md` for the
noninteractive contract and `MANUAL.md` for operational boundaries.
