#!/bin/bash

set -eu

my_dir=$(cd $(dirname $0); pwd)
target_cmd=${E2E_TARGET:-${my_dir}/../target/debug/serverify}
tester_cmd="${E2E_TESTER:-texest}"

have_error=no
for file in $my_dir/cases/*.yaml; do
  echo $(basename ${file})
  SERVERIFY="${target_cmd}" "${tester_cmd}" "${file}" || have_error=yes
  echo
done

test "${have_error}" = 'no'
