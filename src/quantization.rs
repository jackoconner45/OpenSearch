use half::f16;

pub fn quantize_f32_to_f16(embeddings: &[f32]) -> Vec<u8> {
    let mut quantized = Vec::with_capacity(embeddings.len() * 2);
    
    for &value in embeddings {
        let f16_value = f16::from_f32(value);
        let bytes = f16_value.to_bits().to_le_bytes();
        quantized.extend_from_slice(&bytes);
    }
    
    quantized
}

pub fn dequantize_f16_to_f32(quantized: &[u8]) -> Vec<f32> {
    let mut embeddings = Vec::with_capacity(quantized.len() / 2);
    
    for chunk in quantized.chunks_exact(2) {
        let bits = u16::from_le_bytes([chunk[0], chunk[1]]);
        let f16_value = f16::from_bits(bits);
        embeddings.push(f16_value.to_f32());
    }
    
    embeddings
}

pub fn memory_savings(original_dims: usize) -> (usize, usize, f64) {
    let f32_bytes = original_dims * 4;
    let f16_bytes = original_dims * 2;
    let savings_percent = ((f32_bytes - f16_bytes) as f64 / f32_bytes as f64) * 100.0;
    
    (f32_bytes, f16_bytes, savings_percent)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_quantization_roundtrip() {
        let original = vec![1.0, -0.5, 0.0, 3.14159];
        let quantized = quantize_f32_to_f16(&original);
        let dequantized = dequantize_f16_to_f32(&quantized);
        
        // Check approximate equality (f16 has less precision)
        for (orig, deq) in original.iter().zip(dequantized.iter()) {
            assert!((orig - deq).abs() < 0.001, "Original: {}, Dequantized: {}", orig, deq);
        }
    }
    
    #[test]
    fn test_memory_savings() {
        let (f32_bytes, f16_bytes, savings) = memory_savings(768);
        assert_eq!(f32_bytes, 3072);
        assert_eq!(f16_bytes, 1536);
        assert_eq!(savings, 50.0);
    }
}
