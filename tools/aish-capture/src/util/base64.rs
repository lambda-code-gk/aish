// RFC4648 base64 encoding
const BASE64_TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(data: &[u8]) -> String {
    let mut result = String::new();
    let mut i = 0;
    
    while i < data.len() {
        let rem = data.len() - i;
        let b1 = data[i];
        let b2 = if rem > 1 { data[i + 1] } else { 0 };
        let b3 = if rem > 2 { data[i + 2] } else { 0 };
        
        let chunk = (u32::from(b1) << 16) | (u32::from(b2) << 8) | u32::from(b3);
        
        result.push(BASE64_TABLE[((chunk >> 18) & 0x3F) as usize] as char);
        result.push(BASE64_TABLE[((chunk >> 12) & 0x3F) as usize] as char);
        
        if rem == 1 {
            result.push('=');
            result.push('=');
        } else if rem == 2 {
            result.push(BASE64_TABLE[((chunk >> 6) & 0x3F) as usize] as char);
            result.push('=');
        } else {
            result.push(BASE64_TABLE[((chunk >> 6) & 0x3F) as usize] as char);
            result.push(BASE64_TABLE[(chunk & 0x3F) as usize] as char);
        }
        
        i += 3;
    }
    
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_encode() {
        assert_eq!(encode(b"hello"), "aGVsbG8=");
        assert_eq!(encode(b"h"), "aA==");
        assert_eq!(encode(b"he"), "aGU=");
    }
}

