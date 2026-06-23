//! Generated gRPC bindings for the STT sidecar contract.
//!
//! The code is generated at build time by `tonic-build` from
//! `packages/proto/stt.proto` (see `build.rs`) and included here.

pub mod stt {
    #![allow(clippy::all, missing_docs)]
    tonic::include_proto!("lashon.stt.v1");
}

#[cfg(test)]
mod tests {
    use super::stt;

    #[test]
    fn generated_types_construct_and_carry_hebrew() {
        // Proves the build-time codegen ran and the package path is correct.
        let _request = stt::HealthCheckRequest {};
        let response = stt::HealthCheckResponse {
            status: stt::ServingStatus::Serving as i32,
            detail: "סוכן הדיבור פעיל".to_string(),
            version: "0.0.0-m0".to_string(),
            model_ready: false,
        };
        assert_eq!(response.detail, "סוכן הדיבור פעיל");
        assert_eq!(stt::ServingStatus::Serving as i32, 1);
        assert_eq!(stt::ServingStatus::Unspecified as i32, 0);
    }
}
