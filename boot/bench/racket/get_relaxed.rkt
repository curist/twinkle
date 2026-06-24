#lang racket/base
;; Racket treelist mirror of boot/bench/get_relaxed.tw
;; N strided treelist-ref on a concat-built treelist (17-element chunks force
;; relaxed/non-32-aligned seam nodes). Build is outside the timed region.
;;   racket boot/bench/racket/get_relaxed.rkt
(require racket/treelist)

(define (build-concat n)
  (let loop ([i 0] [acc (treelist)])
    (cond
      [(>= i n) acc]
      [else
       (define m (if (<= (+ i 17) n) 17 (- n i)))
       (define chunk
         (for/fold ([c (treelist)]) ([j (in-range m)])
           (treelist-add c (+ i j))))
       (loop (+ i m) (treelist-append acc chunk))])))

(define (bench n)
  (define v (build-concat n))
  (define t0 (current-inexact-milliseconds))
  (let loop ([k 0] [idx 0] [sum 0])
    (cond
      [(= k n)
       (define dt (- (current-inexact-milliseconds) t0))
       (printf "~a\t~a\t~a\n" n dt sum)]
      [else
       (define nidx (modulo (+ idx 40503) n))
       (loop (add1 k) nidx (+ sum (treelist-ref v nidx)))])))

(printf "N\tms\tsink\n")
(for ([n (in-list '(1000 2000 4000 8000 16000 32000))])
  (bench n))
