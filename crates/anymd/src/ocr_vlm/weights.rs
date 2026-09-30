// Immutable model revisions and SHA-256 digests. Documents never leave the machine.
pub const REVISION: &str = "paddleocr-vl-1.6-c5630aba-layout-97d101e6";
pub struct Weight {
    pub path: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
}
pub const FILES: &[Weight] = &[
    Weight { path: "vlm/config.json", url: "https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.6/resolve/c5630abae1d940eafe0697512a0325494b02ab42/config.json", sha256: "ce7f4565f8b1db78532ad5d1b9ebe55c2139d49bd4cb04778b580a08a598f171", bytes: 2059 },
    Weight { path: "vlm/preprocessor_config.json", url: "https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.6/resolve/c5630abae1d940eafe0697512a0325494b02ab42/preprocessor_config.json", sha256: "111872ab1e8bb7fd040ac5087bfced7ab8f011f02139b088cba294964c3b1d0e", bytes: 641 },
    Weight { path: "vlm/chat_template.jinja", url: "https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.6/resolve/c5630abae1d940eafe0697512a0325494b02ab42/chat_template.jinja", sha256: "2f27812dab7f333e471884e0c803d807f11953d5453140dfb1aaba234f872bc8", bytes: 1474 },
    Weight { path: "vlm/model.safetensors", url: "https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.6/resolve/c5630abae1d940eafe0697512a0325494b02ab42/model.safetensors", sha256: "85a479d506a11e724e7285d395c551be69f41dbc16b6342d3cacfb189aed71db", bytes: 1917255968 },
    Weight { path: "vlm/tokenizer.json", url: "https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.6/resolve/c5630abae1d940eafe0697512a0325494b02ab42/tokenizer.json", sha256: "c8a215a59183d0d0781adc33bacd3ce6162716f7fd568fb30234a74d69803a7d", bytes: 11189060 },
    Weight { path: "layout/config.json", url: "https://huggingface.co/PaddlePaddle/PP-DocLayoutV3_safetensors/resolve/97d101e6db2642e162a1d05392d1b0231c91033e/config.json", sha256: "3cf834b91d23a756b1519bce4db42c09e852f3e35c35092dd5a3e253a50c071a", bytes: 2460 },
    Weight { path: "layout/preprocessor_config.json", url: "https://huggingface.co/PaddlePaddle/PP-DocLayoutV3_safetensors/resolve/97d101e6db2642e162a1d05392d1b0231c91033e/preprocessor_config.json", sha256: "519fe0187a43a1ca429e3ad8317bab8700f0d5e8fb3a6e3a0a413ffac078ba42", bytes: 575 },
    Weight { path: "layout/model.safetensors", url: "https://huggingface.co/PaddlePaddle/PP-DocLayoutV3_safetensors/resolve/97d101e6db2642e162a1d05392d1b0231c91033e/model.safetensors", sha256: "5ea422c6cc5fe759a47e1357c35639b58173508e025a3131cbe4b6ac59e2b85e", bytes: 133270468 },
];
