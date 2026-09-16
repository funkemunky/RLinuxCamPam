use pam_linuxcampam::ipc_protocol::{
    command_to_string, string_to_command, Command, Request,
};

#[test]
fn command_to_string_test() {
    assert_eq!(command_to_string(Command::AuthRequest), "AUTH_REQUEST");
    assert_eq!(command_to_string(Command::AddUser), "ADD_USER");
    assert_eq!(command_to_string(Command::Unknown), "UNKNOWN");
}

#[test]
fn string_to_command_test() {
    assert_eq!(string_to_command("AUTH_REQUEST"), Command::AuthRequest);
    assert_eq!(string_to_command("ADD_USER"), Command::AddUser);
    assert_eq!(string_to_command("INVALID_CMD"), Command::Unknown);
}

#[test]
fn serialize_request() {
    let req = Request::new(Command::AuthRequest, vec!["user1".to_string()]);
    assert_eq!(req.serialize(), "AUTH_REQUEST user1");

    let req2 = Request::new(
        Command::TrainUser,
        vec!["user2".to_string(), "label".to_string()],
    );
    assert_eq!(req2.serialize(), "TRAIN_USER user2 label");

    let req3 = Request::new(Command::GetVersion, vec![]);
    assert_eq!(req3.serialize(), "GET_VERSION");
}

#[test]
fn deserialize_request() {
    let req = Request::deserialize("AUTH_REQUEST user1");
    assert_eq!(req.cmd, Command::AuthRequest);
    assert_eq!(req.args.len(), 1);
    assert_eq!(req.args[0], "user1");

    let req2 = Request::deserialize("TRAIN_USER user2 label");
    assert_eq!(req2.cmd, Command::TrainUser);
    assert_eq!(req2.args.len(), 2);
    assert_eq!(req2.args[0], "user2");
    assert_eq!(req2.args[1], "label");

    let req3 = Request::deserialize("GET_VERSION");
    assert_eq!(req3.cmd, Command::GetVersion);
    assert!(req3.args.is_empty());
}

#[test]
fn deserialize_edge_cases() {
    let req = Request::deserialize("AUTH_REQUEST   user1  ");
    assert_eq!(req.cmd, Command::AuthRequest);
    assert_eq!(req.args.len(), 1);
    assert_eq!(req.args[0], "user1");

    let req2 = Request::deserialize("");
    assert_eq!(req2.cmd, Command::Unknown);
}
