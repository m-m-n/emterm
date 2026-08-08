use super::*;
use crate::ansi::apc::KittyCommand;

#[test]
fn test_kitty_handler_creation() {
    let handler = KittyHandler::new();
    assert!(handler.images.is_empty());
    assert!(handler.transfers.is_empty());
}

#[test]
fn test_kitty_query() {
    let mut handler = KittyHandler::new();
    let cmd = KittyCommand {
        action: KittyAction::Query,
        ..Default::default()
    };

    let mut next_id = 1;
    let mut next_placement = 1;
    let events = handler.process(&cmd, 0, 0, &mut next_id, &mut next_placement);

    // Query returns QueryResponse + Response events
    assert_eq!(events.len(), 2);
    assert!(matches!(
        events[0],
        ImageEvent::QueryResponse { supported: true }
    ));
    assert!(matches!(events[1], ImageEvent::Response { .. }));
}

#[test]
fn test_kitty_delete_all() {
    let mut handler = KittyHandler::new();
    let cmd = KittyCommand {
        action: KittyAction::Delete,
        delete_target: Some(KittyDeleteTarget::All),
        ..Default::default()
    };

    let mut next_id = 1;
    let mut next_placement = 1;
    let events = handler.process(&cmd, 0, 0, &mut next_id, &mut next_placement);

    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        ImageEvent::Delete {
            target: ImageDelete::All
        }
    ));
}

#[test]
fn test_kitty_put_without_image() {
    let mut handler = KittyHandler::new();
    let cmd = KittyCommand {
        action: KittyAction::Put,
        image_id: Some(999),
        ..Default::default()
    };

    let mut next_id = 1;
    let mut next_placement = 1;
    let events = handler.process(&cmd, 5, 10, &mut next_id, &mut next_placement);

    // Returns error response (image doesn't exist)
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ImageEvent::Response { .. }));
}

#[test]
fn test_kitty_chunked_transfer() {
    let mut handler = KittyHandler::new();

    // First chunk
    let cmd1 = KittyCommand {
        action: KittyAction::Transmit,
        image_id: Some(1),
        format: Some(KittyFormat::Png),
        more: true,
        payload: "iVBORw".to_string(),
        ..Default::default()
    };

    let mut next_id = 1;
    let mut next_placement = 1;
    let events1 = handler.process(&cmd1, 0, 0, &mut next_id, &mut next_placement);
    assert!(events1.is_empty()); // No event until complete

    // Transfer should be stored
    assert!(handler.transfers.contains_key(&1));

    // Final chunk (invalid PNG data, will fail decode)
    let cmd2 = KittyCommand {
        action: KittyAction::Transmit,
        image_id: Some(1),
        more: false,
        payload: "0KGgo=".to_string(),
        ..Default::default()
    };

    let events2 = handler.process(&cmd2, 0, 0, &mut next_id, &mut next_placement);
    // Decode will fail, returns error response
    assert_eq!(events2.len(), 1);
    assert!(matches!(events2[0], ImageEvent::Response { .. }));
    // Transfer should be cleared
    assert!(!handler.transfers.contains_key(&1));
}

#[test]
fn test_kitty_reset() {
    let mut handler = KittyHandler::new();

    // Add some state
    handler.transfers.insert(
        1,
        ImageTransfer {
            data: "test".to_string(),
            format: None,
            compression: None,
            width: None,
            height: None,
            quiet: None,
        },
    );

    handler.reset();

    assert!(handler.images.is_empty());
    assert!(handler.transfers.is_empty());
}

// =========================================================================
// Response Tests
// =========================================================================

#[test]
fn test_kitty_response_ok() {
    let response = KittyResponse::ok(Some(42), Some(5));
    assert!(response.ok);
    assert_eq!(response.image_id, Some(42));
    assert_eq!(response.placement_id, Some(5));

    let seq = response.to_escape_sequence();
    assert!(seq.contains("i=42"));
    assert!(seq.contains("p=5"));
    assert!(seq.contains("OK"));
}

#[test]
fn test_kitty_response_error() {
    let response = KittyResponse::error(Some(42), KittyErrorCode::ENOENT, "Image not found");
    assert!(!response.ok);
    assert_eq!(response.error_code, Some(KittyErrorCode::ENOENT));

    let seq = response.to_escape_sequence();
    assert!(seq.contains("i=42"));
    assert!(seq.contains("ERROR:ENOENT"));
}

#[test]
fn test_kitty_response_suppression() {
    let ok_response = KittyResponse::ok(Some(1), None);
    let error_response = KittyResponse::error(Some(1), KittyErrorCode::EINVAL, "test");

    // q=1 suppresses OK responses only
    assert!(ok_response.should_suppress(Some(1)));
    assert!(!error_response.should_suppress(Some(1)));

    // q=2 suppresses ALL responses (both OK and ERROR)
    assert!(ok_response.should_suppress(Some(2)));
    assert!(error_response.should_suppress(Some(2)));

    // No quiet mode — nothing suppressed
    assert!(!ok_response.should_suppress(None));
    assert!(!error_response.should_suppress(None));
}

#[test]
fn test_kitty_error_codes_display() {
    assert_eq!(KittyErrorCode::EINVAL.to_string(), "EINVAL");
    assert_eq!(KittyErrorCode::ENOENT.to_string(), "ENOENT");
    assert_eq!(KittyErrorCode::ENOSPC.to_string(), "ENOSPC");
    assert_eq!(KittyErrorCode::EFAILED.to_string(), "EFAILED");
}
