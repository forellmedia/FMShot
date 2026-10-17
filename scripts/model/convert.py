"""Wandelt SigLIP 2 (fp32-ONNX) in die mitgelieferte, kleinere Fassung (ADR 0010).

- Bild: alle MatMul-Gewichte 8 Bit (nur Gewichte, Rechnung in int8) – 108 MB,
  Treffer wie das volle Modell.
- Text: Wortschatz-Tabelle (Gather) 8 Bit, MatMul-Gewichte 4 Bit – 250 MB.

Aufruf über scripts/fetch-model.sh (mit festen Paketversionen über uv).
"""

import sys

import onnx
from onnxruntime.quantization import QuantType, quantize_dynamic
from onnxruntime.quantization.matmul_nbits_quantizer import (
    DefaultWeightOnlyQuantConfig,
    MatMulNBitsQuantizer,
)


def weight_only(src: str, dst: str, bits: int) -> None:
    config = DefaultWeightOnlyQuantConfig(block_size=32, is_symmetric=True, bits=bits, accuracy_level=4)
    quantizer = MatMulNBitsQuantizer(onnx.load(src), algo_config=config)
    quantizer.process()
    quantizer.model.save_model_to_file(dst, use_external_data_format=False)


def main(cache: str, out: str) -> None:
    weight_only(f"{cache}/vision_model.onnx", f"{out}/vision.onnx", bits=8)
    gather = f"{cache}/text_gather8.onnx"
    quantize_dynamic(
        f"{cache}/text_model.onnx",
        gather,
        op_types_to_quantize=["Gather"],
        per_channel=False,
        reduce_range=False,
        weight_type=QuantType.QInt8,
    )
    weight_only(gather, f"{out}/text.onnx", bits=4)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
