VERSION=$(shell grep ^version Cargo.toml|cut -d\" -f2)

all:
	@echo "Select target"

prepare:
	sudo apt -y install libqt5webkit5-dev qttools5-dev qtbase5-dev-tools libqt5charts5-dev libssl-dev pkg-config g++ cmake

release: pkg upload-pkg

upload-pkg:
	gsutil -m cp -a public-read target/pkg/* gs://pub.bma.ai/ecmui/${VERSION}/
	jks build pub.bma.ai

pkg: check-ver clean-pkg build-deb-u20 build-deb-u22 build-msi

check-ver:
	if ! curl -sI https://pub.bma.ai/ecmui/${VERSION}/ecmui-${VERSION}-x86_64.msi 2>&1 \
		|head -1|grep " 404 " > /dev/null; then  echo "Version already exists"; exit 1; fi

clean-pkg:
	rm -rf target/pkg

build-deb-u20:
	mkdir -p target/pkg
	ssh -t lab-builder1 ". ~/.cargo/env && cd /build/ecmui && git checkout Cargo.lock && git pull && make prepare && cargo build --release && cargo bundle --release"
	scp lab-builder1:/build/ecmui/target/release/bundle/deb/ecmui_${VERSION}_amd64.deb ./target/pkg/ecmui_${VERSION}_ubuntu20.04_amd64.deb

build-deb-u22:
	mkdir -p target/pkg
	ssh -t lab-builder-u22 ". ~/.cargo/env && cd /build/ecmui && git checkout Cargo.lock && git pull && make prepare && cargo build --release && cargo bundle --release"
	scp lab-builder-u22:/build/ecmui/target/release/bundle/deb/ecmui_${VERSION}_amd64.deb ./target/pkg/ecmui_${VERSION}_ubuntu22.04_amd64.deb

build-msi:
	mkdir -p target/pkg
	ssh -t lab-vwin1 "cd /src && init.bat && cd /src/ecmui && git checkout Cargo.lock && git pull && cargo build --release && cargo wix"
	scp lab-vwin1:/src/ecmui/target/wix/ecmui-${VERSION}-x86_64.msi ./target/pkg/
