cargo build --release --target wasm32-unknown-unknown -p mitex-typst
rm typst-package/mitex.wasm -Force
mv target/wasm32-unknown-unknown/release/mitex_typst.wasm typst-package/mitex.wasm