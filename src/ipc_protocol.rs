#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    AuthRequest,
    AddUser,
    TrainUser,
    TestAuth,
    SetLabel,
    TrainNew,
    ListEmbeddings,
    RemoveEmbedding,
    GetConfig,
    SetLogLevel,
    GetLogLevel,
    GetVersion,
    Unknown,
}

impl Command {
    pub fn as_str(&self) -> &'static str {
        match self {
            Command::AuthRequest => "AUTH_REQUEST",
            Command::AddUser => "ADD_USER",
            Command::TrainUser => "TRAIN_USER",
            Command::TestAuth => "TEST_AUTH",
            Command::SetLabel => "SET_LABEL",
            Command::TrainNew => "TRAIN_NEW",
            Command::ListEmbeddings => "LIST_EMBEDDINGS",
            Command::RemoveEmbedding => "REMOVE_EMBEDDING",
            Command::GetConfig => "GET_CONFIG",
            Command::SetLogLevel => "SET_LOG_LEVEL",
            Command::GetLogLevel => "GET_LOG_LEVEL",
            Command::GetVersion => "GET_VERSION",
            Command::Unknown => "UNKNOWN",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "AUTH_REQUEST" => Command::AuthRequest,
            "ADD_USER" => Command::AddUser,
            "TRAIN_USER" => Command::TrainUser,
            "TEST_AUTH" => Command::TestAuth,
            "SET_LABEL" => Command::SetLabel,
            "TRAIN_NEW" => Command::TrainNew,
            "LIST_EMBEDDINGS" => Command::ListEmbeddings,
            "REMOVE_EMBEDDING" => Command::RemoveEmbedding,
            "GET_CONFIG" => Command::GetConfig,
            "SET_LOG_LEVEL" => Command::SetLogLevel,
            "GET_LOG_LEVEL" => Command::GetLogLevel,
            "GET_VERSION" => Command::GetVersion,
            _ => Command::Unknown,
        }
    }
}

impl std::str::FromStr for Command {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Command::from_str(s))
    }
}

pub fn command_to_string(cmd: Command) -> String {
    cmd.as_str().to_string()
}

pub fn string_to_command(str: &str) -> Command {
    Command::from_str(str)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub cmd: Command,
    pub args: Vec<String>,
}

impl Request {
    pub fn new(cmd: Command, args: Vec<String>) -> Self {
        Self { cmd, args }
    }

    pub fn serialize(&self) -> String {
        let mut out = command_to_string(self.cmd);
        for arg in &self.args {
            out.push(' ');
            out.push_str(arg);
        }
        out
    }

    pub fn deserialize(data: &str) -> Self {
        let (cmd_sv, rest) = match data.find(' ') {
            Some(idx) => (&data[..idx], Some(&data[idx + 1..])),
            None => (data, None),
        };

        let cmd = string_to_command(cmd_sv);
        let mut args = Vec::new();

        if let Some(rest) = rest {
            for token in rest.split(' ') {
                if !token.is_empty() {
                    args.push(token.to_string());
                }
            }
        }

        Self { cmd, args }
    }
}
