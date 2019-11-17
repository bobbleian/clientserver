use std::collections::HashSet;

pub struct GameData {
    player_names: Vec<String>,
    player_ids: Vec<usize>,
    restart_ids: HashSet<usize>,
    game_board: Vec<u8>,
    active_player: u8,
    max_players: u8,
    max_move: u8,
    game_board_size: u8,
    state: Option<Box<dyn GameState>>,
}

impl GameData {
    pub fn new(max_players: u8, max_move: u8, game_board_size: u8) -> GameData {
        GameData {
            player_names: Vec::new(),
            player_ids: Vec::<usize>::new(),
            restart_ids: HashSet::<usize>::new(),
            game_board: Vec::new(),
            active_player: std::u8::MAX,
            max_players: max_players,
            max_move: max_move,
            game_board_size: game_board_size,
            state: Some(Box::new(WaitingForPlayers {})),
        }
    }

    pub fn add_player(&mut self, player_id: usize, player_name: &str) {
        if let Some(s) = self.state.take() {
            self.state = Some(s.add_player(self, player_id, player_name));
        }
    }

    pub fn set_active_player(&mut self, player_id: usize) {
        if let Some(player) = self.player_ids.iter().position(|id| *id == player_id) {
            if let Some(s) = self.state.take() {
                self.state = Some(s.set_active_player(self, player as u8));
            }
        } else {
            panic!("Cannot find player: {}", player_id);
        }
    }

    pub fn move_player(&mut self, player_id: usize, player_move: u8) {
        println!("Moving player {} {} steps", player_id, player_move);
        if let Some(player) = self.player_ids.iter().position(|id| *id == player_id) {
            if let Some(s) = self.state.take() {
                self.state = Some(s.move_player(self, player as u8, player_move));
            }
        } else {
            panic!("Cannot find player: {}", player_id);
        }
    }

    pub fn is_game_over(&self) -> bool {
        if let Some(ref s) = self.state {
            return s.is_game_over();
        }
        false
    }

    pub fn game_has_player(&self, player_id: usize) -> bool {
        self.player_ids.contains(&player_id)
    }

    pub fn get_max_move(&self) -> u8 { self.max_move }

    pub fn get_active_player_id(&self) -> u8 {
        match self.player_ids.get(self.active_player as usize) {
            Some(&i) => { i as u8 },
            None => { std::u8::MAX },
        }
    }

    pub fn get_game_board(&self) -> &[u8] { self.game_board.as_slice() }

    pub fn get_player_names(&self) -> &Vec<String> { &self.player_names }

}

trait GameState {
    fn add_player(self: Box<Self>, game_data: &mut GameData, player_id: usize, player_name: &str) -> Box<dyn GameState>;
    fn move_player(self: Box<Self>, game_data: &mut GameData, player: u8, player_move: u8) -> Box<dyn GameState>;
    fn set_active_player(self: Box<Self>, game_data: &mut GameData, player: u8) -> Box<dyn GameState>;
    fn is_game_over(&self) -> bool { return false; }
}

struct WaitingForPlayers {
}

struct WaitingOnMove {
}

struct GameOver {
}

impl GameState for WaitingForPlayers {
    fn add_player(self: Box<Self>, game_data: &mut GameData, player_id: usize, player_name: &str) -> Box<dyn GameState> {
        println!("Adding player_id={}; player_name={}", player_id, player_name);
        game_data.player_names.push(player_name.to_string());
        game_data.player_ids.push(player_id);
        if game_data.player_names.len() as u8 >= game_data.max_players {
            // Randomly select an active player
            game_data.active_player = 0;

            // Update the game state
            Box::new(WaitingOnMove {})
        } else {
            self
        }
    }

    // Empty implementation
    fn move_player(self: Box<Self>, _game_data: &mut GameData, _player: u8, _player_move: u8) -> Box<dyn GameState> { self }
    fn set_active_player(self: Box<Self>, _game_data: &mut GameData, _player: u8) -> Box<dyn GameState> { self }
    fn is_game_over(&self) -> bool { return false; }
}

impl GameState for WaitingOnMove {
    // Empty implementation
    fn add_player(self: Box<Self>, _game_data: &mut GameData, _player_id: usize, _player_name: &str) -> Box<dyn GameState> {
        self
    }

    fn move_player(self: Box<Self>, game_data: &mut GameData, player: u8, player_move: u8) -> Box<dyn GameState> {
        println!("Game in progress, moving player");
        // Check the active player is the one who is making the move
        if player != game_data.active_player {
            println!("Player is not the active player, skipping move");
            return self;
        }

        // Check the player is making a valid move
        if player_move > game_data.max_move || player_move < 1 {
            println!("Invalid move {}; max move is {}", player_move, game_data.max_move);
            return self;
        }

        for _ in 0..player_move {
            game_data.game_board.push(player);
        }

        // Echo game state
        println!("Game total is: {}", game_data.game_board.len());

        // Check for loser
        if game_data.game_board.len() as u8 >= game_data.game_board_size {
            println!("{} has lost the game!!!", game_data.player_names.get(player as usize).unwrap());
            return Box::new(GameOver {});
        }

        // Game continues, next player's move
        game_data.active_player = (game_data.active_player + 1)%(game_data.player_names.len() as u8);
        self
    }

    fn set_active_player(self: Box<Self>, game_data: &mut GameData, player: u8) -> Box<dyn GameState> {
        // Only allow a change in active player if no moves have been made
        if game_data.game_board.len() == 0 {
            game_data.active_player = player;
        }
        self
    }

    fn is_game_over(&self) -> bool { return false; }

}

impl GameState for GameOver {
    fn add_player(self: Box<Self>, game_data: &mut GameData, player_id: usize, player_name: &str) -> Box<dyn GameState> {
        println!("Adding player_id={}; player_name={}", player_id, player_name);
        game_data.restart_ids.insert(player_id);
        if game_data.player_ids.len() == game_data.restart_ids.len() {
            // Restart the game - reset game state
            game_data.restart_ids.clear();
            game_data.game_board.clear();

            // Leave starting player - last game's loser

            // Update the game state
            Box::new(WaitingOnMove {})
        } else {
            self
        }
    }

    // Empty implementation
    fn move_player(self: Box<Self>, _game_data: &mut GameData, _player: u8, _player_move: u8) -> Box<dyn GameState> {
        println!("Game Over, cannot move player");
        self
    }

    fn set_active_player(self: Box<Self>, _game_data: &mut GameData, _player: u8) -> Box<dyn GameState> { self }

    // Game IS over!
    fn is_game_over(&self) -> bool { return true; }
}


pub enum Message {
    // Client Messages
    ClientUserName = 0,
    PlayerMove = 1,
    RestartGame = 2,
    EndGame = 3,

    // Server Messages
    OpponentDisconnect = 128,
    GameData = 4,
    AddPlayer = 5,
    MovePlayer = 6,
    SetActivePlayer = 7,
    Welcome = 8,
    ServerUserName = 9,
}

impl Message {
    pub fn from_u8(value: u8) -> Option<Message> {
        match value {
            // Client Messages
            0 => Some(Message::ClientUserName),
            1 => Some(Message::PlayerMove),
            2 => Some(Message::RestartGame),
            3 => Some(Message::EndGame),

            // Server Messages
            128 => Some(Message::OpponentDisconnect),  // Changed from 0
            4 => Some(Message::GameData),
            5 => Some(Message::AddPlayer),
            6 => Some(Message::MovePlayer),
            7 => Some(Message::SetActivePlayer),
            8 => Some(Message::Welcome),
            9 => Some(Message::ServerUserName),

            // Not Found
            _ => None,
        }
    }
}

