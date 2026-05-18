use rand::Rng;

pub struct Map {
    pub grid: Vec<Vec<u8>>,
    pub width: usize,
    pub height: usize,
    pub spawn: (f64, f64),
    #[allow(dead_code)]
    pub exit: (usize, usize),
    pub enemy_spawns: Vec<(f64, f64)>,
}

impl Map {
    pub fn generate<R: Rng>(rng: &mut R) -> Self {
        let maze_w: usize = 12;
        let maze_h: usize = 12;
        let grid_w = maze_w * 2 + 1;
        let grid_h = maze_h * 2 + 1;

        let mut grid = vec![vec![1u8; grid_w]; grid_h];
        let mut visited = vec![vec![false; maze_w]; maze_h];
        let mut stack: Vec<(usize, usize)> = Vec::new();

        // Open start room and begin DFS
        grid[1][1] = 0;
        visited[0][0] = true;
        stack.push((0, 0));

        let directions: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

        while !stack.is_empty() {
            let &(cx, cy) = stack.last().unwrap();

            let mut neighbors: Vec<(usize, usize, usize, usize)> = Vec::new();
            for &(dx, dy) in &directions {
                let nx = cx as i32 + dx;
                let ny = cy as i32 + dy;
                if nx >= 0 && nx < maze_w as i32 && ny >= 0 && ny < maze_h as i32 {
                    let (nx, ny) = (nx as usize, ny as usize);
                    if !visited[ny][nx] {
                        // Wall between (cx,cy) and (nx,ny) in grid coords
                        let wall_gx = cx + nx + 1;
                        let wall_gy = cy + ny + 1;
                        neighbors.push((nx, ny, wall_gx, wall_gy));
                    }
                }
            }

            if neighbors.is_empty() {
                stack.pop();
            } else {
                let idx = rng.gen_range(0..neighbors.len());
                let (nx, ny, wall_gx, wall_gy) = neighbors[idx];
                grid[wall_gy][wall_gx] = 0;
                grid[ny * 2 + 1][nx * 2 + 1] = 0;
                visited[ny][nx] = true;
                stack.push((nx, ny));
            }
        }

        // Mark exit cell (far corner room)
        let exit_gx = (maze_w - 1) * 2 + 1;
        let exit_gy = (maze_h - 1) * 2 + 1;
        grid[exit_gy][exit_gx] = 3;

        // Collect enemy spawn positions: open rooms away from start
        let mut enemy_spawns = Vec::new();
        for cy in 0..maze_h {
            for cx in 0..maze_w {
                let dist = cx + cy; // Manhattan distance from (0,0)
                if dist >= 4 && rng.gen_bool(0.22) {
                    let gx = cx * 2 + 1;
                    let gy = cy * 2 + 1;
                    if grid[gy][gx] == 0 {
                        enemy_spawns.push((gx as f64 + 0.5, gy as f64 + 0.5));
                    }
                }
            }
        }
        if enemy_spawns.len() < 3 {
            enemy_spawns.push((5.5, 5.5));
            enemy_spawns.push((9.5, 5.5));
            enemy_spawns.push((5.5, 9.5));
        }

        Map {
            grid,
            width: grid_w,
            height: grid_h,
            spawn: (1.5, 1.5),
            exit: (exit_gx, exit_gy),
            enemy_spawns,
        }
    }

    #[inline]
    pub fn is_wall(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return true;
        }
        self.grid[y as usize][x as usize] == 1
    }

    #[inline]
    pub fn is_exit(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return false;
        }
        self.grid[y as usize][x as usize] == 3
    }

    #[inline]
    pub fn cell(&self, x: i32, y: i32) -> u8 {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return 1;
        }
        self.grid[y as usize][x as usize]
    }
}
