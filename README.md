# gabriel

Measures how many unspent public key addresses there are, and how many coins are in them over time. Early Satoshi-era coins that are just sitting with exposed public keys. If we see lots of coins move... That's a potential sign that quantum computers have silently broken bitcoin.

## Execution

### Pre-reqs

```
$ bitcoind \
    -conf=$GITEA_HOME/blockchain/bitcoin/admin/bitcoind/bitcoin.conf \
    -daemon=0
```

#### Hardware

#### Software
##### Rust
The best way to install Rust is to use [rustup](https://rustup.rs).

##### bitcoind

#### Environment Variables

### Clone code

```
$ git clone https://github.com/SurmountSystems/gabriel.git
$ git checkout HB/gabriel-v2
```

### Build

* execute:

        $ cargo build

* view gabriel command line options:


        $ ./target/debug/gabriel

### Execute tests

```
$ cargo test
```

### Run Gabriel Indexer


* execute indexer:

        $ export BITCOIND_DATA_DIR=/path/to/bitcoind/data/dir
        $ ./target/debug/gabriel index \
            --input $BITCOIND_DATA_DIR/blocks \
            --output /tmp/gabriel-testnet4.csv

#### Debug in VSCode:

Add and edit the following to $PROJECT_HOME/.vscode/launch.json:

`````
{
    "version": "0.2.0",
    "configurations": [
        {
            "type": "lldb",
            "request": "launch",
            "name": "Debug 'index' capability of executable 'gabriel'",
            "args": ["index", "--input=/u02/bitcoin/datadir/testnet4/blocks", "--output=/tmp/gabriel-testnet4.csv"],
            "cwd": "${workspaceFolder}",
            "program": "./target/debug/gabriel",
            "sourceLanguages": ["rust"]
        }
    ]
}
`````
