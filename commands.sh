rustup target add armv7-unknown-linux-gnueabihf  # Raspberry Pi 4
sudo apt install gcc-arm-linux-gnueabihf  # Install GCC cross-compiler
cargo build --release --target=armv7-unknown-linux-gnueabihf  # Cross-build an optimised executable
rsync ./target/armv7-unknown-linux-gnueabihf/release/main $TARGET_HOST:${TARGET_PATH}
