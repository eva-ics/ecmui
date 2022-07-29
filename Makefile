VERSION=$(shell grep ^version Cargo.toml|cut -d\" -f2)

all:
	@echo "Select target"

prepare:
	sudo apt -y install libqt5webkit5-dev qttools5-dev qtbase5-dev-tools libqt5charts5-dev libssl-dev pkg-config g++ cmake

pkg: clean-pkg build-deb build-msi

clean-pkg:
	rm -rf target/pkg

build-deb:
	mkdir -p target/pkg
	ssh -t lab-builder1 ". ~/.cargo/env && cd /build/ecmui && git checkout Cargo.lock && git pull && make prepare && cargo build --release && cargo bundle --release"
	scp lab-builder1:/build/ecmui/target/release/bundle/deb/ecmui_${VERSION}_amd64.deb ./target/pkg/

build-msi:
	mkdir -p target/pkg
	ssh -t lab-win1 "cd /src && init.bat && cd /src/ecmui && git checkout Cargo.lock && git pull && cargo build --release && cargo wix"
	scp lab-win1:/src/ecmui/target/wix/ecmui-${VERSION}-x86_64.msi ./target/pkg/
