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

pub fn decode(data: &str) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    let mut i = 0;
    let bytes = data.as_bytes();
    
    while i < bytes.len() {
        if bytes[i] == b'=' {
            break;
        }
        
        let mut chunk = 0u32;
        let mut count = 0;
        
        while count < 4 && i < bytes.len() {
            let c = bytes[i];
            if c == b'=' {
                break;
            }
            
            let value = match c {
                b'A'..=b'Z' => (c - b'A') as u32,
                b'a'..=b'z' => (c - b'a' + 26) as u32,
                b'0'..=b'9' => (c - b'0' + 52) as u32,
                b'+' => 62,
                b'/' => 63,
                _ => return Err(format!("Invalid base64 character: {}", c as char)),
            };
            
            chunk = (chunk << 6) | value;
            count += 1;
            i += 1;
        }
        
        match count {
            2 => {
                result.push((chunk >> 4) as u8);
            }
            3 => {
                result.push((chunk >> 10) as u8);
                result.push((chunk >> 2) as u8);
            }
            4 => {
                result.push((chunk >> 16) as u8);
                result.push((chunk >> 8) as u8);
                result.push(chunk as u8);
            }
            _ => {}
        }
        
        // Skip padding
        while i < bytes.len() && bytes[i] == b'=' {
            i += 1;
        }
    }
    
    Ok(result)
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
    
    #[test]
    fn test_decode() {
        assert_eq!(decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(decode("aA==").unwrap(), b"h");
        assert_eq!(decode("aGU=").unwrap(), b"he");
    }
}

