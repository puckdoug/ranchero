default: check test

check:
  cargo check
  cargo clippy

test:
  cargo test

build:
  @cargo build

clean:
  @echo "clean"

realclean: clean
  @rm -rf target

superclean: realclean
  @rm -rf ./tmp/* ./tmp/.*

# --------------------------------------------------------------------
# helpers for running ranchero
# --------------------------------------------------------------------

# Run ranchero - accepts all arguments
ranchero *ARGV:
  cargo run -- {{ ARGV }}

# ranchero help
help:
  cargo run -- help

# start ranchero in debug mode (foreground) and create capture file
debug:
  cargo run -- start --debug --capture tmp/output.cap

# start ranchero in follow mode reading capture file
follow:
  cargo run -- follow tmp/output.cap
