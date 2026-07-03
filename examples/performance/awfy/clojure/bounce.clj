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

;; Unlocked tier: 100 balls packed into a native long-array (4 fields each),
;; mutated in place — Clojure's escape hatch to the native league. The hot loop
;; lives in a helper with a `^longs`-hinted parameter so array reads stay
;; unboxed (the hint does not survive across nested `loop` closures otherwise);
;; this is what tuned Clojure array code actually requires.
(defn ^long bounce-mut-once [^longs balls]
  (loop [k 0 seed 74755]
    (when (< k 100)
      (let [s1 (bounce-lcg seed) s2 (bounce-lcg s1) s3 (bounce-lcg s2) s4 (bounce-lcg s3)
            b (* k 4)]
        (aset-long balls b (mod s1 500))
        (aset-long balls (+ b 1) (mod s2 500))
        (aset-long balls (+ b 2) (- (mod s3 5) 2))
        (aset-long balls (+ b 3) (- (mod s4 5) 2))
        (recur (inc k) s4))))
  (let [bounces
        (loop [s 0 bc 0]
          (if (< s 50)
            (recur (inc s)
                   (loop [j 0 bc bc]
                     (if (< j 100)
                       (let [b (* j 4)
                             x (aget balls b) y (aget balls (+ b 1))
                             xv (aget balls (+ b 2)) yv (aget balls (+ b 3))
                             nx (+ x xv) ny (+ y yv)
                             over-x (> nx 500) under-x (< nx 0)
                             nnx (cond over-x 500 under-x 0 :else nx)
                             nxv (if (or over-x under-x) (- xv) xv)
                             over-y (> ny 500) under-y (< ny 0)
                             nny (cond over-y 500 under-y 0 :else ny)
                             nyv (if (or over-y under-y) (- yv) yv)]
                         (aset-long balls b nnx) (aset-long balls (+ b 1) nny)
                         (aset-long balls (+ b 2) nxv) (aset-long balls (+ b 3) nyv)
                         (recur (inc j) (if (or over-x under-x over-y under-y) (inc bc) bc)))
                       bc)))
            bc))]
    (loop [j 0 acc bounces]
      (if (< j 100)
        (recur (inc j) (+ acc (aget balls (* j 4)) (aget balls (+ (* j 4) 1))))
        acc))))

(defn bounce-mut-run [size]
  (let [balls (long-array 400)]
    (loop [rep 0 total 0]
      (if (< rep size)
        (recur (inc rep) (bounce-mut-once balls))
        total))))

(def bounce-mut-bench
  {:name "bounce_mut" :warmup 10 :iters 20 :size 400 :expected 47174 :run bounce-mut-run})
