#!/usr/bin/env python3
"""Generate a 'fine-tuned' variant of mlp_mnist.onnx for delta demo.

Perturbs only the last fully-connected layer (fc3.weight, fc3.bias),
simulating transfer learning. The rest of the model is identical byte-for-byte.
"""
import numpy as np
import onnx
import sys

src = "models/test/mlp_mnist.onnx"
dst = "models/test/mlp_mnist_v2.onnx"

model = onnx.load(src)

modified = 0
for init in model.graph.initializer:
    if init.name in ("w3", "b3"):
        arr = np.frombuffer(init.raw_data, dtype=np.float32).copy()
        # Small perturbation: add ~1% noise
        noise = np.random.default_rng(42).normal(0, 0.01 * np.std(arr), arr.shape).astype(np.float32)
        arr += noise
        init.raw_data = arr.tobytes()
        modified += 1
        print(f"  perturbed {init.name}: {arr.shape} ({len(init.raw_data)} bytes)")

if modified == 0:
    print("ERROR: no tensors modified — check tensor names", file=sys.stderr)
    sys.exit(1)

onnx.save(model, dst)
print(f"\nSaved {dst} ({len(open(dst, 'rb').read())} bytes)")
print(f"Only last layer changed — delta should be tiny")
