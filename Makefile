DOCKER_IMAGE := wasabi-dev:latest
DOCKER_PLATFORM ?= linux/amd64
DOCKER_BUILD_FLAGS ?= --platform=$(DOCKER_PLATFORM)
DOCKER_RUN_FLAGS ?= --platform=$(DOCKER_PLATFORM)
ESP_DIR := mnt
VNC_PORT ?= 5900
VNC_PASSWORD ?= test

.PHONY: docker-build
docker-build: ## Build the dev container image.
	docker build $(DOCKER_BUILD_FLAGS) -t $(DOCKER_IMAGE) .

.PHONY: linux-shell
linux-shell: docker-build ## Start an interactive shell in the dev container.
	docker run --rm -it --privileged $(DOCKER_RUN_FLAGS) \
		-v "$(PWD)":/workspace -w /workspace \
		$(DOCKER_IMAGE)

.PHONY: qemu-linux
qemu-linux: docker-build ## Build the UEFI binary, create the ESP, and run QEMU in the container (VNC:5900 exposed).
	docker run --rm -it --privileged $(DOCKER_RUN_FLAGS) -p $(VNC_PORT):5900 \
		-v "$(PWD)":/workspace -w /workspace \
		$(DOCKER_IMAGE) \
		bash -c 'set -euo pipefail; cargo build --target x86_64-unknown-uefi; make qemu VNC_PASSWORD="$(VNC_PASSWORD)"'

.PHONY: qemu-linux-vnc
qemu-linux-vnc: ## Run qemu-linux and open the local VNC client automatically.
	( sleep 3; $(MAKE) vnc-open VNC_PORT="$(VNC_PORT)" ) &
	$(MAKE) qemu-linux VNC_PORT="$(VNC_PORT)" VNC_PASSWORD="$(VNC_PASSWORD)"

.PHONY: linux-test
linux-test: docker-build ## Run cargo test inside the dev container.
	docker run --rm -it --privileged $(DOCKER_RUN_FLAGS) \
		-v "$(PWD)":/workspace -w /workspace \
		$(DOCKER_IMAGE) \
		cargo test

.PHONY: vnc-open
vnc-open: ## Open VNC client to vnc://localhost:5900.
	@if command -v open >/dev/null 2>&1; then \
		open vnc://localhost:$(VNC_PORT); \
	elif command -v xdg-open >/dev/null 2>&1; then \
		xdg-open vnc://localhost:$(VNC_PORT); \
	else \
		echo "Open vnc://localhost:$(VNC_PORT) manually."; \
	fi

.PHONY: uefi-esp
uefi-esp: ## Create a FAT ESP with the built UEFI binary at EFI/BOOT/BOOTX64.EFI.
	mkdir -p $(ESP_DIR)/EFI/BOOT
	cp target/x86_64-unknown-uefi/debug/wasabi.efi $(ESP_DIR)/EFI/BOOT/BOOTX64.EFI

.PHONY: qemu
qemu: uefi-esp ## Run QEMU with OVMF, VNC (localhost:5900), VGA device, serial+monitor on stdio.
# VNC password is configured at startup (no monitor `set_password` needed).
	QEMU_AUDIO_DRV=none qemu-system-x86_64 \
		-bios third_party/ovmf/RELEASEX64_OVMF.fd \
		-M q35 -m 2G -smp 4 \
		-accel tcg,thread=multi \
		-drive format=raw,file=fat:rw:$(ESP_DIR),if=ide,media=disk \
		-device VGA -display none \
		-object secret,id=vncpass,data=$(VNC_PASSWORD) \
		-vnc :0,password-secret=vncpass \
		-serial mon:stdio
