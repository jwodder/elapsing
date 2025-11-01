#!/usr/bin/env python3
import sys
import time

for i, line in enumerate(sys.stdin, start=1):
    print(f"Line {i}: {line.strip()}", flush=True)
    time.sleep(1)
