local pi = 3.141592653589793
local solar_mass = 4 * pi * pi
local days_per_year = 365.24

local function init_bodies()
  local bodies = {
    { x = 0, y = 0, z = 0, vx = 0, vy = 0, vz = 0, mass = solar_mass },
    { x = 4.84143144246472090e+00, y = -1.16032004402742839e+00, z = -1.03622044471123109e-01,
      vx = 1.66007664274403694e-03 * days_per_year, vy = 7.69901118419740425e-03 * days_per_year, vz = -6.90460016972063023e-05 * days_per_year,
      mass = 9.54791938424326609e-04 * solar_mass },
    { x = 8.34336671824457987e+00, y = 4.12479856412430479e+00, z = -4.03523417114321381e-01,
      vx = -2.76742510726862411e-03 * days_per_year, vy = 4.99852801234917238e-03 * days_per_year, vz = 2.30417297573763929e-05 * days_per_year,
      mass = 2.85885980666130812e-04 * solar_mass },
    { x = 1.28943695621391310e+01, y = -1.51111514016986312e+01, z = -2.23307578892655734e-01,
      vx = 2.96460137564761618e-03 * days_per_year, vy = 2.37847173959480950e-03 * days_per_year, vz = -2.96589568540237556e-05 * days_per_year,
      mass = 4.36624404335156298e-05 * solar_mass },
    { x = 1.53796971148509165e+01, y = -2.59193146099879641e+01, z = 1.79258772950371181e-01,
      vx = 2.68067772490389322e-03 * days_per_year, vy = 1.62824170038242295e-03 * days_per_year, vz = -9.51592254519715870e-05 * days_per_year,
      mass = 5.15138902046611451e-05 * solar_mass },
  }
  local px, py, pz = 0, 0, 0
  for _, body in ipairs(bodies) do
    px, py, pz = px + body.vx * body.mass, py + body.vy * body.mass, pz + body.vz * body.mass
  end
  bodies[1].vx, bodies[1].vy, bodies[1].vz = -px / solar_mass, -py / solar_mass, -pz / solar_mass
  return bodies
end

local function advance(bodies, dt)
  for i = 1, #bodies do
    local first = bodies[i]
    for j = i + 1, #bodies do
      local second = bodies[j]
      local dx, dy, dz = first.x - second.x, first.y - second.y, first.z - second.z
      local squared = dx * dx + dy * dy + dz * dz
      local magnitude = dt / (squared * math.sqrt(squared))
      first.vx, first.vy, first.vz = first.vx - dx * second.mass * magnitude, first.vy - dy * second.mass * magnitude, first.vz - dz * second.mass * magnitude
      second.vx, second.vy, second.vz = second.vx + dx * first.mass * magnitude, second.vy + dy * first.mass * magnitude, second.vz + dz * first.mass * magnitude
    end
  end
  for _, body in ipairs(bodies) do
    body.x, body.y, body.z = body.x + dt * body.vx, body.y + dt * body.vy, body.z + dt * body.vz
  end
end

local function energy(bodies)
  local result = 0
  for i = 1, #bodies do
    local first = bodies[i]
    result = result + 0.5 * first.mass * (first.vx * first.vx + first.vy * first.vy + first.vz * first.vz)
    for j = i + 1, #bodies do
      local second = bodies[j]
      local dx, dy, dz = first.x - second.x, first.y - second.y, first.z - second.z
      result = result - first.mass * second.mass / math.sqrt(dx * dx + dy * dy + dz * dz)
    end
  end
  return result
end

return {
  warmup = 5,
  iters = 20,
  size = 20000,
  expected = -16908926,
  run = function(size)
    local bodies = init_bodies()
    for _ = 1, size do advance(bodies, 0.01) end
    return math.floor(energy(bodies) * 1e8 + 0.5)
  end,
}
