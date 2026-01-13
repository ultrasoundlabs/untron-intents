forge build --root packages/contracts
forge bind  --root packages/contracts \
  --crate-name untron-intents-bindings \
  --bindings-path crates/bindings \
  --overwrite
