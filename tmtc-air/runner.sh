#/bin/sh

TOKEN_ARGS=""
if [ -f flatsat.token ]; then
  TOKEN_ARGS="--host ws://open-lst-1.flatsat.space --token $(cat flatsat.token)"
  #TOKEN_ARGS="--host ws://open-lst-2.flatsat.space --token $(cat flatsat_lst2.token)"
fi

exec probe-rs run --chip STM32H723VG $TOKEN_ARGS "$@"
