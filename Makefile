DOCKER_IMAGE := wasabi-dev:latest
DOCKER_PLATFORM ?= linux/amd64
VNC_PORT ?= 5900
VNC_PASSWORD ?= test
PROFILE ?= debug
OBJDUMP_ARGS ?= --disassemble --no-show-raw-insn

.PHONY: docker-build
docker-build: ## Build the dev container image.
	docker build --platform=$(DOCKER_PLATFORM) -t $(DOCKER_IMAGE) .

.PHONY: qemu
qemu: docker-build ## Build the UEFI binary and run QEMU in the dev container (VNC:5900 exposed).
	docker run --rm -it --privileged --platform=$(DOCKER_PLATFORM) -p $(VNC_PORT):5900 \
		-v "$(PWD)":/workspace -w /workspace \
		$(DOCKER_IMAGE) \
		bash -c 'set -euo pipefail; \
			cargo build --target x86_64-unknown-uefi; \
			mkdir -p mnt/EFI/BOOT; \
			cp target/x86_64-unknown-uefi/debug/wasabi.efi mnt/EFI/BOOT/BOOTX64.EFI; \
			QEMU_AUDIO_DRV=none qemu-system-x86_64 \
				-bios third_party/ovmf/RELEASEX64_OVMF.fd \
				-M q35 -m 4G -smp 4 \
				-accel tcg,thread=multi \
				-drive format=raw,file=fat:rw:mnt,if=ide,media=disk \
				-device VGA -display none \
				-object secret,id=vncpass,data=$(VNC_PASSWORD) \
				-vnc :0,password-secret=vncpass \
				-serial mon:stdio'

.PHONY: test
test: docker-build ## Run cargo test inside the dev container.
	docker run --rm -it --privileged --platform=$(DOCKER_PLATFORM) \
		-v "$(PWD)":/workspace -w /workspace \
		$(DOCKER_IMAGE) \
		cargo test

.PHONY: objdump
objdump: docker-build ## Build the UEFI binary and disassemble it with cargo-objdump in the dev container.
	docker run --rm -it --privileged --platform=$(DOCKER_PLATFORM) \
		-v "$(PWD)":/workspace -w /workspace \
		$(DOCKER_IMAGE) \
		bash -c 'set -euo pipefail; \
			args=""; \
			if [ "$(PROFILE)" = "release" ]; then \
				args="$$args --release"; \
			fi; \
			cargo objdump --target x86_64-unknown-uefi --bin wasabi $$args -- $(OBJDUMP_ARGS)'

.PHONY: vnc-open
vnc-open: ## Open VNC client to vnc://localhost:5900.
	@if command -v open >/dev/null 2>&1; then \
		open vnc://localhost:$(VNC_PORT); \
	elif command -v xdg-open >/dev/null 2>&1; then \
		xdg-open vnc://localhost:$(VNC_PORT); \
	else \
		echo "Open vnc://localhost:$(VNC_PORT) manually."; \
	fi
