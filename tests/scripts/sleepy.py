#!/usr/bin/env python3
import sys
import time

time.sleep(1)
print("Starting...", flush=True)
time.sleep(2)
print("Working...", flush=True)
if sys.stdout.isatty():
    print("Stdout is a tty", flush=True)
else:
    print("Stdout is not a tty", flush=True)
time.sleep(2)
print("Shutting down...", flush=True)
time.sleep(1)
