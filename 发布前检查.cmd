@echo off
chcp 65001
cd wftpd
echo 工作目录：wftpd
cargo fmt --check
echo 格式检查完成

cargo clippy --release
echo Clippy 检查完成

cargo check --release
echo 检查完成

cargo build --release
echo 构建完成

cd ../wftpg
echo 工作目录：wftpg

cargo fmt --check
echo 格式检查完成

cargo clippy --release
echo Clippy 检查完成

cargo check --release
echo 检查完成

cargo build --release
echo 构建完成
