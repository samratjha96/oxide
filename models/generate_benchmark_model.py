#!/usr/bin/env python3
"""Generate a larger test ONNX model for realistic benchmarking.
Creates an MLP with ~500K parameters (image feature extractor + classifier).
"""

import numpy as np
import onnx
from onnx import TensorProto, helper, numpy_helper
import os

MODEL_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "test")


def make_mlp_model():
    """
    MLP model simulating a small image feature classifier:
    Input: [1, 784] (28x28 flattened image)
    Hidden1: 512 neurons + ReLU
    Hidden2: 256 neurons + ReLU
    Output: [1, 10] (10 classes) with Softmax
    Total parameters: ~784*512 + 512*256 + 256*10 ≈ 535K
    """
    X = helper.make_tensor_value_info("input", TensorProto.FLOAT, [1, 784])
    Y = helper.make_tensor_value_info("output", TensorProto.FLOAT, [1, 10])

    np.random.seed(42)
    
    # Layer 1: 784 -> 512
    W1 = numpy_helper.from_array(
        (np.random.randn(784, 512).astype(np.float32) * 0.05), name="w1"
    )
    B1 = numpy_helper.from_array(np.zeros(512, dtype=np.float32), name="b1")

    # Layer 2: 512 -> 256
    W2 = numpy_helper.from_array(
        (np.random.randn(512, 256).astype(np.float32) * 0.05), name="w2"
    )
    B2 = numpy_helper.from_array(np.zeros(256, dtype=np.float32), name="b2")

    # Layer 3: 256 -> 10
    W3 = numpy_helper.from_array(
        (np.random.randn(256, 10).astype(np.float32) * 0.05), name="w3"
    )
    B3 = numpy_helper.from_array(np.zeros(10, dtype=np.float32), name="b3")

    nodes = [
        helper.make_node("MatMul", ["input", "w1"], ["h1_mm"], name="fc1"),
        helper.make_node("Add", ["h1_mm", "b1"], ["h1"], name="fc1_bias"),
        helper.make_node("Relu", ["h1"], ["h1_relu"], name="relu1"),
        helper.make_node("MatMul", ["h1_relu", "w2"], ["h2_mm"], name="fc2"),
        helper.make_node("Add", ["h2_mm", "b2"], ["h2"], name="fc2_bias"),
        helper.make_node("Relu", ["h2"], ["h2_relu"], name="relu2"),
        helper.make_node("MatMul", ["h2_relu", "w3"], ["h3_mm"], name="fc3"),
        helper.make_node("Add", ["h3_mm", "b3"], ["h3"], name="fc3_bias"),
        helper.make_node("Softmax", ["h3"], ["output"], axis=1, name="softmax"),
    ]

    graph = helper.make_graph(
        nodes, "mlp_classifier", [X], [Y],
        initializer=[W1, B1, W2, B2, W3, B3]
    )
    model = helper.make_model(graph, opset_imports=[helper.make_opsetid("", 13)])
    model.ir_version = 7

    path = os.path.join(MODEL_DIR, "mlp_mnist.onnx")
    onnx.save(model, path)
    size = os.path.getsize(path)
    params = 784*512 + 512 + 512*256 + 256 + 256*10 + 10
    print(f"  Created: {path}")
    print(f"  Size: {size:,} bytes ({size/1024:.1f} KB)")
    print(f"  Parameters: {params:,}")
    return path


if __name__ == "__main__":
    os.makedirs(MODEL_DIR, exist_ok=True)
    print("Generating benchmark ONNX model...")
    make_mlp_model()
    print("Done!")
