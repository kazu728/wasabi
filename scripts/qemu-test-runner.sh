#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <test-efi-path>" >&2
  exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
TEST_EFI="$1"
ESP_DIR="${REPO_ROOT}/mnt"
OVMF_PATH="${REPO_ROOT}/third_party/ovmf/RELEASEX64_OVMF.fd"
TEST_OK_MARKER="WASABI_TEST_RESULT:OK"
TEST_FAIL_MARKER="WASABI_TEST_RESULT:FAIL"
QEMU_TIMEOUT_SECONDS="${QEMU_TIMEOUT_SECONDS:-300}"

mkdir -p "${ESP_DIR}/EFI/BOOT"
cp "${TEST_EFI}" "${ESP_DIR}/EFI/BOOT/BOOTX64.EFI"

log_file="$(mktemp)"
cleanup() {
  rm -f "${log_file}"
}
trap cleanup EXIT

QEMU_AUDIO_DRV=none qemu-system-x86_64 \
  -bios "${OVMF_PATH}" \
  -M q35 -m 512M -smp 2 \
  -accel tcg,thread=multi \
  -drive format=raw,file=fat:rw:${ESP_DIR},if=ide,media=disk \
  -display none \
  -serial stdio \
  -monitor none \
  -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
  -no-reboot \
  -no-shutdown \
  >"${log_file}" 2>&1 &
qemu_pid=$!

tail -n +1 -f --pid="${qemu_pid}" "${log_file}" &
tail_pid=$!

saw_ok=0
saw_fail=0
timed_out=0
start_ts="$(date +%s)"

while kill -0 "${qemu_pid}" 2>/dev/null; do
  if grep -Fq "${TEST_FAIL_MARKER}" "${log_file}"; then
    saw_fail=1
    kill "${qemu_pid}" 2>/dev/null || true
    break
  fi

  if grep -Fq "${TEST_OK_MARKER}" "${log_file}"; then
    saw_ok=1
    kill "${qemu_pid}" 2>/dev/null || true
    break
  fi

  now_ts="$(date +%s)"
  if (( now_ts - start_ts >= QEMU_TIMEOUT_SECONDS )); then
    timed_out=1
    echo "qemu-test-runner: timed out after ${QEMU_TIMEOUT_SECONDS}s" >&2
    kill "${qemu_pid}" 2>/dev/null || true
    break
  fi

  sleep 0.1
done

set +e
wait "${qemu_pid}"
qemu_status=$?
wait "${tail_pid}" >/dev/null 2>&1
set -e

# QEMU isa-debug-exit returns (value << 1) | 1.
# 0x10 => 33 is treated as test success.
if [[ "${qemu_status}" -eq 33 ]]; then
  exit 0
fi

if [[ "${saw_fail}" -eq 1 ]]; then
  exit 1
fi

if [[ "${saw_ok}" -eq 1 ]]; then
  exit 0
fi

if [[ "${timed_out}" -eq 1 ]]; then
  exit 124
fi

exit "${qemu_status}"
