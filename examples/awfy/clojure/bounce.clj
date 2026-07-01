;; Bounce over a Clojure persistent vector of immutable ball vectors. Each step
;; rebinds the outer vector with `assoc` (persistent write) — the structure
;; under comparison. AWFY's LCG is ported exactly for identical checksums.
(defn bounce-lcg [seed]
  (bit-and (+ (* seed 1309) 13849) 65535))

(defn bounce-init-balls []
  (loop [k 0 seed 74755 balls []]
    (if (< k 100)
      (let [s1 (bounce-lcg seed)
            s2 (bounce-lcg s1)
            s3 (bounce-lcg s2)
            s4 (bounce-lcg s3)
            b [(mod s1 500) (mod s2 500) (- (mod s3 5) 2) (- (mod s4 5) 2)]]
        (recur (inc k) s4 (conj balls b)))
      balls)))

;; Returns [new-ball bounced?].
(defn bounce-step [b]
  (let [x (nth b 0) y (nth b 1) xv (nth b 2) yv (nth b 3)
        nx (+ x xv) ny (+ y yv)
        [nx xv b1] (cond (> nx 500) [500 (- xv) true]
                         (< nx 0)   [0 (- xv) true]
                         :else      [nx xv false])
        [ny yv b2] (cond (> ny 500) [500 (- yv) true]
                         (< ny 0)   [0 (- yv) true]
                         :else      [ny yv b1])]
    [[nx ny xv yv] b2]))

(defn bounce-run [size]
  (loop [rep 0 total 0]
    (if (< rep size)
      (let [balls0 (bounce-init-balls)
            [final-balls bounces]
            (loop [s 0 balls balls0 bounces 0]
              (if (< s 50)
                (let [[balls' bounces']
                      (loop [j 0 bs balls bc bounces]
                        (if (< j 100)
                          (let [[nb hit] (bounce-step (nth bs j))]
                            (recur (inc j) (assoc bs j nb) (if hit (inc bc) bc)))
                          [bs bc]))]
                  (recur (inc s) balls' bounces'))
                [balls bounces]))
            checksum (reduce (fn [acc b] (+ acc (nth b 0) (nth b 1)))
                             bounces final-balls)]
        (recur (inc rep) checksum))
      total)))

(def bounce-bench
  {:name "bounce" :warmup 10 :iters 20 :size 400 :expected 47174 :run bounce-run})
