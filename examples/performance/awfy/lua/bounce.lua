local compat = ...

local function make_rng()
  local seed = 74755
  return function()
    seed = compat.band(seed * 1309 + 13849, 65535)
    return seed
  end
end

local function new_ball(next_value)
  return {
    x = next_value() % 500,
    y = next_value() % 500,
    xv = (next_value() % 5) - 2,
    yv = (next_value() % 5) - 2,
  }
end

local function bounce_step(ball)
  local bounced = false
  local nx, ny, xv, yv = ball.x + ball.xv, ball.y + ball.yv, ball.xv, ball.yv
  if nx > 500 then nx, xv, bounced = 500, -xv, true end
  if nx < 0 then nx, xv, bounced = 0, -xv, true end
  if ny > 500 then ny, yv, bounced = 500, -yv, true end
  if ny < 0 then ny, yv, bounced = 0, -yv, true end
  ball.x, ball.y, ball.xv, ball.yv = nx, ny, xv, yv
  return bounced
end

return {
  warmup = 10,
  iters = 20,
  size = 400,
  expected = 47174,
  run = function(size)
    local total = 0
    for _ = 1, size do
      local next_value, balls = make_rng(), {}
      for i = 1, 100 do balls[i] = new_ball(next_value) end
      local bounces = 0
      for _ = 1, 50 do
        for i = 1, 100 do
          if bounce_step(balls[i]) then bounces = bounces + 1 end
        end
      end
      total = bounces
      for i = 1, 100 do total = total + balls[i].x + balls[i].y end
    end
    return total
  end,
}
