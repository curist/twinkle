#lang racket/base
;; Racket treelist mirror of boot/bench/concat_append.tw
;; Append-at-end concat: (treelist-append acc (treelist i)). Linear control case.
;;   racket boot/bench/racket/concat_append.rkt
(require racket/treelist)

(define (bench n)
  (define t0 (current-inexact-milliseconds))
  (let loop ([i 0] [acc (treelist)])
    (cond
      [(= i n)
       (define dt (- (current-inexact-milliseconds) t0))
       (printf "~a\t~a\t~a\n" n dt (treelist-ref acc (sub1 (treelist-length acc))))]
      [else
       (loop (add1 i) (treelist-append acc (treelist i)))])))

(printf "N\tms\tsink\n")
(for ([n (in-list '(1000 2000 4000 8000 16000 32000))])
  (bench n))
