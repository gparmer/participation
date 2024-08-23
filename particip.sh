#!/bin/bash

set -e

ROSTER_PATH=~/docs/teaching/f24_3411/roster.csv
NEW_ROSTER_PATH=${ROSTER_PATH}.out

usage()
{
    echo "Usage: " $0 " run | rotate | rm"
    exit 1
}

case $1 in
    run )
	cargo run $ROSTER_PATH
	;;
    rotate )
	date=$(date +"%Y-%m-%d")
	# save the old one
	mv $ROSTER_PATH $date.$ROSTER_PATH
	# update it to the new one reflecting updates
	mv $NEW_ROSTER_PATH $ROSTER_PATH
	;;
    rm )
	# remove the new, updated file...useful for testing
	rm ${ROSTER_PATH}.out
	;;
    * )
	usage
esac
