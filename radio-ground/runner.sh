#/bin/sh

TOKEN_ARGS=""
if [[ -f flatsat.token && -z "${FLASH_LOCAL+x}" ]]; then
  TOKEN_ARGS="--host ws://ground-lst.flatsat.space --token $(cat flatsat.token)"
fi

exec probe-rs run --chip STM32H723VG $TOKEN_ARGS "$@"
