#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    if let Ok(message)=gcu_host_protocol::codec::validate(data) {
        assert!(message.validate().is_ok());
        if let gcu_host_protocol::messages::Envelope::Request(request)=message {
            let _=gcu_host_protocol::messages::validate_params(&request);
        }
    }
});
