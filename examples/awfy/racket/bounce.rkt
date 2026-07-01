#lang racket/base
;; Bounce over a Racket treelist of immutable ball vectors. Each step rebinds
;; the outer treelist with `treelist-set` (persistent write). AWFY's LCG is
;; ported exactly for identical checksums.
(require racket/treelist "harness.rkt")
(provide bounce-bench)

(define (bounce-lcg seed) (bitwise-and (+ (* seed 1309) 13849) 65535))

(define (bounce-init-balls)
  (let loop ([k 0] [seed 74755] [balls (treelist)])
    (if (< k 100)
        (let* ([s1 (bounce-lcg seed)] [s2 (bounce-lcg s1)] [s3 (bounce-lcg s2)] [s4 (bounce-lcg s3)]
               [b (vector (modulo s1 500) (modulo s2 500) (- (modulo s3 5) 2) (- (modulo s4 5) 2))])
          (loop (add1 k) s4 (treelist-add balls b)))
        balls)))

;; Returns (values new-ball bounced?).
(define (bounce-step b)
  (define x (vector-ref b 0)) (define y (vector-ref b 1))
  (define xv (vector-ref b 2)) (define yv (vector-ref b 3))
  (define nx (+ x xv)) (define ny (+ y yv))
  (define bounced #f)
  (define-values (nx2 xv2)
    (cond [(> nx 500) (set! bounced #t) (values 500 (- xv))]
          [(< nx 0)   (set! bounced #t) (values 0 (- xv))]
          [else (values nx xv)]))
  (define-values (ny2 yv2)
    (cond [(> ny 500) (set! bounced #t) (values 500 (- yv))]
          [(< ny 0)   (set! bounced #t) (values 0 (- yv))]
          [else (values ny yv)]))
  (values (vector nx2 ny2 xv2 yv2) bounced))

(define (ball-pos-sum balls)
  (let loop ([i 0] [acc 0])
    (if (< i (treelist-length balls))
        (let ([b (treelist-ref balls i)])
          (loop (add1 i) (+ acc (vector-ref b 0) (vector-ref b 1))))
        acc)))

(define (bounce-run size)
  (let rep-loop ([rep 0] [total 0])
    (if (< rep size)
        (let ([balls0 (bounce-init-balls)])
          (let step-loop ([s 0] [balls balls0] [bounces 0])
            (if (< s 50)
                (let ball-loop ([j 0] [bs balls] [bc bounces])
                  (if (< j 100)
                      (let-values ([(nb hit) (bounce-step (treelist-ref bs j))])
                        (ball-loop (add1 j) (treelist-set bs j nb) (if hit (add1 bc) bc)))
                      (step-loop (add1 s) bs bc)))
                (rep-loop (add1 rep) (+ bounces (ball-pos-sum balls))))))
        total)))

(define bounce-bench (bench "bounce" 10 20 400 47174 bounce-run))
