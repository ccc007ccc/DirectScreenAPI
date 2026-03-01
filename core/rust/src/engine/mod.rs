mod protocol;
mod runtime;

pub use protocol::{
    execute_binary_command, parse_binary_command, parse_binary_header, BinaryCommand,
    BinaryCommandHeader, BinaryDisplaySetPayload, BinaryOpcode, BinaryRenderSubmitPayload,
    BinaryResponse, BinaryTouchPayload, BINARY_COMMAND_HEADER_BYTES, BINARY_PROTOCOL_MAGIC,
    BINARY_PROTOCOL_VERSION, BINARY_RESPONSE_PAYLOAD_BYTES, BINARY_RESPONSE_VALUE_COUNT,
};
pub use runtime::{RenderPresentInfo, RuntimeEngine};
