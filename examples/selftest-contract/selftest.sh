#!/bin/sh
# Replace these with real checks that drive your app or game headlessly.
fail=0
check() { if eval "$2"; then echo "PASS $1"; else echo "FAIL $1"; fail=1; fi; }
check "snake moves right by default"   "true"
check "eating food grows the snake"    "true"
check "hitting a wall ends the game"   "true"
check "score increments per food"      "true"
exit $fail
