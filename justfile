default: help

help:
  cargo run -- help

debug:
  cargo run -- start --debug --capture tmp/output.cap

follow: 
  cargo run -- follow tmp/output.cap
