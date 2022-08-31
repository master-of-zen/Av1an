#!/bin/bash

rm master.json zerocopy.json

./target/release/av1an -i "$1" --sc-method fast --sc-only -s zerocopy.json
/usr/bin/av1an -i "$1" --sc-method fast --sc-only -s master.json

cmp master.json zerocopy.json
