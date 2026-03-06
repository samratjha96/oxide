#!/usr/bin/env python3
"""Generate test ONNX models for Oxide integration testing.

Creates several small ONNX models:
1. add_model.onnx - Simple add operation (input + 3)
2. linear_model.onnx - Linear layer (matmul + bias)
3. classifier_model.onnx - Small image classifier (conv + relu + fc)
4. multi_io_model.onnx - Multiple inputs and outputs
"""

import numpy as np
import onnx
from onnx import TensorProto, helper, numpy_helper
import os

MODEL_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "test")


def make_add_model():
    """Model that adds 3.0 to each element of input."""
    X = helper.make_tensor_value_info("input", TensorProto.FLOAT, [1, 4])
    Y = helper.make_tensor_value_info("output", TensorProto.FLOAT, [1, 4])

    three = numpy_helper.from_array(np.array([3.0, 3.0, 3.0, 3.0], dtype=np.float32), name="three")

    add_node = helper.make_node("Add", inputs=["input", "three"], outputs=["output"], name="add")

    graph = helper.make_graph([add_node], "add_graph", [X], [Y], initializer=[three])
    model = helper.make_model(graph, opset_imports=[helper.make_opsetid("", 13)])
    model.ir_version = 7

    path = os.path.join(MODEL_DIR, "add_model.onnx")
    onnx.save(model, path)
    print(f"  Created: {path} ({os.path.getsize(path)} bytes)")
    return path


def make_linear_model():
    """Simple linear model: output = input @ weights + bias."""
    X = helper.make_tensor_value_info("input", TensorProto.FLOAT, [1, 8])
    Y = helper.make_tensor_value_info("output", TensorProto.FLOAT, [1, 4])

    # Random weights
    np.random.seed(42)
    W_data = np.random.randn(8, 4).astype(np.float32) * 0.1
    B_data = np.random.randn(4).astype(np.float32) * 0.01

    W = numpy_helper.from_array(W_data, name="weights")
    B = numpy_helper.from_array(B_data, name="bias")

    matmul = helper.make_node("MatMul", ["input", "weights"], ["matmul_out"], name="matmul")
    add = helper.make_node("Add", ["matmul_out", "bias"], ["output"], name="add_bias")

    graph = helper.make_graph([matmul, add], "linear_graph", [X], [Y], initializer=[W, B])
    model = helper.make_model(graph, opset_imports=[helper.make_opsetid("", 13)])
    model.ir_version = 7

    path = os.path.join(MODEL_DIR, "linear_model.onnx")
    onnx.save(model, path)
    print(f"  Created: {path} ({os.path.getsize(path)} bytes)")
    return path


def make_classifier_model():
    """Small classifier: flatten -> linear -> relu -> linear -> softmax."""
    X = helper.make_tensor_value_info("input", TensorProto.FLOAT, [1, 16])
    Y = helper.make_tensor_value_info("output", TensorProto.FLOAT, [1, 4])

    np.random.seed(123)
    W1_data = np.random.randn(16, 8).astype(np.float32) * 0.1
    B1_data = np.zeros(8, dtype=np.float32)
    W2_data = np.random.randn(8, 4).astype(np.float32) * 0.1
    B2_data = np.zeros(4, dtype=np.float32)

    W1 = numpy_helper.from_array(W1_data, name="w1")
    B1 = numpy_helper.from_array(B1_data, name="b1")
    W2 = numpy_helper.from_array(W2_data, name="w2")
    B2 = numpy_helper.from_array(B2_data, name="b2")

    nodes = [
        helper.make_node("MatMul", ["input", "w1"], ["fc1_out"], name="fc1"),
        helper.make_node("Add", ["fc1_out", "b1"], ["fc1_bias_out"], name="fc1_bias"),
        helper.make_node("Relu", ["fc1_bias_out"], ["relu_out"], name="relu"),
        helper.make_node("MatMul", ["relu_out", "w2"], ["fc2_out"], name="fc2"),
        helper.make_node("Add", ["fc2_out", "b2"], ["fc2_bias_out"], name="fc2_bias"),
        helper.make_node("Softmax", ["fc2_bias_out"], ["output"], axis=1, name="softmax"),
    ]

    graph = helper.make_graph(nodes, "classifier_graph", [X], [Y], initializer=[W1, B1, W2, B2])
    model = helper.make_model(graph, opset_imports=[helper.make_opsetid("", 13)])
    model.ir_version = 7

    path = os.path.join(MODEL_DIR, "classifier_model.onnx")
    onnx.save(model, path)
    print(f"  Created: {path} ({os.path.getsize(path)} bytes)")
    return path


def make_sigmoid_model():
    """Simplest possible model: sigmoid activation."""
    X = helper.make_tensor_value_info("input", TensorProto.FLOAT, [1, 4])
    Y = helper.make_tensor_value_info("output", TensorProto.FLOAT, [1, 4])

    sigmoid = helper.make_node("Sigmoid", ["input"], ["output"], name="sigmoid")

    graph = helper.make_graph([sigmoid], "sigmoid_graph", [X], [Y])
    model = helper.make_model(graph, opset_imports=[helper.make_opsetid("", 13)])
    model.ir_version = 7

    path = os.path.join(MODEL_DIR, "sigmoid_model.onnx")
    onnx.save(model, path)
    print(f"  Created: {path} ({os.path.getsize(path)} bytes)")
    return path


if __name__ == "__main__":
    os.makedirs(MODEL_DIR, exist_ok=True)
    print("Generating test ONNX models...")
    make_add_model()
    make_linear_model()
    make_classifier_model()
    make_sigmoid_model()
    print("Done! All models saved to:", MODEL_DIR)
