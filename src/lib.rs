pub struct GameData {
    player_names: Vec<String>,
    game_board: Vec<usize>,
    active_player: usize,
    max_players: usize,
    max_move: usize,
    game_board_size: usize,
    state: Option<Box<dyn GameState>>,
}

impl GameData {
    pub fn new(max_players: usize, max_move: usize, game_board_size: usize) -> GameData {
        GameData {
            player_names: Vec::new(),
            game_board: Vec::new(),
            active_player: std::usize::MAX,
            max_players: max_players,
            max_move: max_move,
            game_board_size: game_board_size,
            state: Some(Box::new(WaitingForPlayers {})),
        }
    }

    pub fn add_player(&mut self, player_name: &str) {
        if let Some(s) = self.state.take() {
            self.state = Some(s.add_player(self, player_name));
        }
    }

    pub fn move_player(&mut self, player_name: &String, player_move: usize) {
        println!("Moving player {} {} steps", player_name, player_move);
        if let Some(player) = self.player_names.iter().position(|name| name == player_name) {
            if let Some(s) = self.state.take() {
                self.state = Some(s.move_player(self, player, player_move));
            }
        } else {
            panic!("Cannot find player: {}", player_name);
        }
    }

    pub fn get_player_name(&self, player: usize) -> Option<String> {
        if let Some(player_name) = self.player_names.get(player) {
            Some(player_name.to_string())
        } else {
            None
        }
    }

    pub fn game_has_player(&self, player_name: &String) -> bool {
        self.player_names.contains(player_name)
    }

}

trait GameState {
    fn add_player(self: Box<Self>, game_data: &mut GameData, player_name: &str) -> Box<dyn GameState>;
    fn move_player(self: Box<Self>, game_data: &mut GameData, player: usize, player_move: usize) -> Box<dyn GameState>;
    fn is_game_over() -> bool where Self: Sized { return false; }
}

struct WaitingForPlayers {
}

struct WaitingOnMove {
}

struct GameOver {
}

impl GameState for WaitingForPlayers {
    fn add_player(self: Box<Self>, game_data: &mut GameData, player_name: &str) -> Box<dyn GameState> {
        game_data.player_names.push(player_name.to_string());
        if game_data.player_names.len() >= game_data.max_players {
            // Randomly select an active player
            game_data.active_player = 0;

            // Update the game state
            Box::new(WaitingOnMove {})
        } else {
            self
        }
    }

    // Empty implementation
    fn move_player(self: Box<Self>, _game_data: &mut GameData, _player: usize, _player_move: usize) -> Box<dyn GameState> {
        println!("Waiting for player, no move made");
        self
    }
}

impl GameState for WaitingOnMove {
    // Empty implementation
    fn add_player(self: Box<Self>, _game_data: &mut GameData, _player_name: &str) -> Box<dyn GameState> {
        self
    }

    fn move_player(self: Box<Self>, game_data: &mut GameData, player: usize, player_move: usize) -> Box<dyn GameState> {
        println!("Game in progress, moving player");
        // Check the active player is the one who is making the move
        if player != game_data.active_player {
            println!("Player is not the active player, skipping move");
            return self;
        }

        // Check the player is making a valid move
        if player_move > game_data.max_move {
            println!("Invalid move {}; max move is {}", player_move, game_data.max_move);
            return self;
        }

        for _ in 0..player_move {
            game_data.game_board.push(player);
        }

        // Echo game state
        println!("Game total is: {}", game_data.game_board.len());

        // Check for loser
        if game_data.game_board.len() >= game_data.game_board_size {
            println!("{} has lost the game!!!", game_data.player_names.get(player).unwrap());
            return Box::new(GameOver {});
        }

        // Game continues, next player's move
        game_data.active_player = (game_data.active_player + 1)%game_data.player_names.len();
        self
    }

}

impl GameState for GameOver {
    fn add_player(self: Box<Self>, _game_data: &mut GameData, _player_name: &str) -> Box<dyn GameState> {
        self
    }

    // Empty implementation
    fn move_player(self: Box<Self>, _game_data: &mut GameData, _player: usize, _player_move: usize) -> Box<dyn GameState> {
        println!("Game Over, cannot move player");
        self
    }

    // Game IS over!
    fn is_game_over() -> bool where Self: Sized { return true; }
}


