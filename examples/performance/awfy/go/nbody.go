package main

import "math"

const nbPI = 3.141592653589793
const nbSolarMass = 4 * nbPI * nbPI
const nbDaysPerYear = 365.24

type nbBody struct {
	x, y, z, vx, vy, vz, mass float64
}

func nbInit() []nbBody {
	bodies := []nbBody{
		{mass: nbSolarMass}, // Sun
		{ // Jupiter
			x: 4.84143144246472090, y: -1.16032004402742839, z: -1.03622044471123109e-01,
			vx: 1.66007664274403694e-03 * nbDaysPerYear, vy: 7.69901118419740425e-03 * nbDaysPerYear, vz: -6.90460016972063023e-05 * nbDaysPerYear,
			mass: 9.54791938424326609e-04 * nbSolarMass,
		},
		{ // Saturn
			x: 8.34336671824457987, y: 4.12479856412430479, z: -4.03523417114321381e-01,
			vx: -2.76742510726862411e-03 * nbDaysPerYear, vy: 4.99852801234917238e-03 * nbDaysPerYear, vz: 2.30417297573763929e-05 * nbDaysPerYear,
			mass: 2.85885980666130812e-04 * nbSolarMass,
		},
		{ // Uranus
			x: 1.28943695621391310e+01, y: -1.51111514016986312e+01, z: -2.23307578892655734e-01,
			vx: 2.96460137564761618e-03 * nbDaysPerYear, vy: 2.37847173959480950e-03 * nbDaysPerYear, vz: -2.96589568540237556e-05 * nbDaysPerYear,
			mass: 4.36624404335156298e-05 * nbSolarMass,
		},
		{ // Neptune
			x: 1.53796971148509165e+01, y: -2.59193146099879641e+01, z: 1.79258772950371181e-01,
			vx: 2.68067772490389322e-03 * nbDaysPerYear, vy: 1.62824170038242295e-03 * nbDaysPerYear, vz: -9.51592254519715870e-05 * nbDaysPerYear,
			mass: 5.15138902046611451e-05 * nbSolarMass,
		},
	}
	var px, py, pz float64
	for i := range bodies {
		px += bodies[i].vx * bodies[i].mass
		py += bodies[i].vy * bodies[i].mass
		pz += bodies[i].vz * bodies[i].mass
	}
	bodies[0].vx = -px / nbSolarMass
	bodies[0].vy = -py / nbSolarMass
	bodies[0].vz = -pz / nbSolarMass
	return bodies
}

func nbAdvance(bodies []nbBody, dt float64) {
	n := len(bodies)
	for i := 0; i < n; i++ {
		for j := i + 1; j < n; j++ {
			dx := bodies[i].x - bodies[j].x
			dy := bodies[i].y - bodies[j].y
			dz := bodies[i].z - bodies[j].z
			d2 := dx*dx + dy*dy + dz*dz
			mag := dt / (d2 * math.Sqrt(d2))
			bodies[i].vx -= dx * bodies[j].mass * mag
			bodies[i].vy -= dy * bodies[j].mass * mag
			bodies[i].vz -= dz * bodies[j].mass * mag
			bodies[j].vx += dx * bodies[i].mass * mag
			bodies[j].vy += dy * bodies[i].mass * mag
			bodies[j].vz += dz * bodies[i].mass * mag
		}
	}
	for i := 0; i < n; i++ {
		bodies[i].x += dt * bodies[i].vx
		bodies[i].y += dt * bodies[i].vy
		bodies[i].z += dt * bodies[i].vz
	}
}

func nbEnergy(bodies []nbBody) float64 {
	n := len(bodies)
	e := 0.0
	for i := 0; i < n; i++ {
		bi := bodies[i]
		e += 0.5 * bi.mass * (bi.vx*bi.vx + bi.vy*bi.vy + bi.vz*bi.vz)
		for j := i + 1; j < n; j++ {
			bj := bodies[j]
			dx := bi.x - bj.x
			dy := bi.y - bj.y
			dz := bi.z - bj.z
			dist := math.Sqrt(dx*dx + dy*dy + dz*dz)
			e -= (bi.mass * bj.mass) / dist
		}
	}
	return e
}

func nbodyRun(size int) int {
	bodies := nbInit()
	for i := 0; i < size; i++ {
		nbAdvance(bodies, 0.01)
	}
	return int(math.Round(nbEnergy(bodies) * 1e8))
}

var nbodyBench = Bench{Name: "nbody", Warmup: 5, Iters: 20, Size: 20000, Expected: -16908926, Run: nbodyRun}
