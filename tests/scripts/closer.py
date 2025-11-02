#!/usr/bin/env python3
import os
import sys
import time

print("This is the last time I write to stdout!", flush=True)
os.close(sys.stdout.fileno())
time.sleep(1)
print("And THIS is the last time I write to stderr!", file=sys.stderr, flush=True)
os.close(sys.stderr.fileno())
time.sleep(1)
