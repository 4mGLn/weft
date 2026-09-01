# Getting Started with Weft

1. Extract the release archive and install the binary:

   ```bash
   tar -xzf weft-<version>-x86_64-unknown-linux-gnu.tar.gz
   PREFIX="$HOME/.local" ./weft-<version>-x86_64-unknown-linux-gnu/install.sh
   ```

2. Choose a durable local state directory and initialize it:

   ```bash
   weft --state-dir "$HOME/.local/share/weft" init
   ```

3. Create the first durable Change:

   ```bash
   weft --format json --state-dir "$HOME/.local/share/weft" change create \
     --change-id first-change --operation-id first-change-create \
     --actor your-name --at 1000
   ```

4. Inspect it:

   ```bash
   weft --state-dir "$HOME/.local/share/weft" change show \
     --change-id first-change
   ```

Use an application-generated Unix-millisecond timestamp for production
automation rather than the illustrative `1000` above. See `USAGE.md` for the
noninteractive contract and `MANUAL.md` for operational boundaries.
