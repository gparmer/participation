#!/bin/bash

set -e

ROSTER_PATH=~/docs/teaching/f24_3411/roster.csv

usage()
{
    echo "Usage: " $0 " run | rotate"
    exit 1
}

case $1 in
    run )
	cargo run $ROSTER_PATH
	;;
    rotate )
	mv ${ROSTER_PATH}.out $ROSTER_PATH
	;;
    * )
	usage
esac
