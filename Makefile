all:
	@echo "Select target"

prepare:
	sudo apt -y install libqt5webkit5-dev qttools5-dev qtbase5-dev-tools libqt5charts5-dev libssl-dev pkg-config g++ cmake

build-deb:
	mkdir -p target/pkg
	ssh -t lab-builder1 ". ~/.cargo/env && cd /build/ecmui && git checkout Cargo.lock && git pull && make prepare && cargo build --release && cargo bundle --release"
