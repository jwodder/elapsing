#!/usr/bin/env python3
import sys
import time

print("This goes to stdout.", flush=True)
time.sleep(1)
print("And this goes to stderr.", file=sys.stderr, flush=True)
time.sleep(1)
print("Back to stdout.", flush=True)
