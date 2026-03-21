(module
  (type $rt_types__Array (array (mut anyref)))
  (type $rt_types__String (array (mut i8)))
  (type $rt_types__DictEntry (struct (field $key anyref) (field $val anyref)))
  (type $rt_types__Dict (array (mut (ref null $rt_types__DictEntry))))
  (type $rt_types__ClosureEnv (array anyref))
  (type $rt_types__ClosureFunc (func (param anyref anyref) (result anyref)))
  (type $rt_types__Closure (sub (struct (field $func_ref (ref null $rt_types__ClosureFunc)) (field $env (ref null $rt_types__ClosureEnv)))))
  (type $rt_types__Variant (struct (field $type_id i32) (field $variant_id i32) (field $payload (ref null $rt_types__Array))))
  (type $rt_types__BoxedInt (struct (field $v i64)))
  (type $rt_types__BoxedFloat (struct (field $v f64)))
  (type $rt_types__IterState (sub (struct (field $seed anyref) (field $step anyref))))
  (type $user__UserRecord_2 (struct))
  (type $user__UserRecord_3 (struct (field $f0 (mut i64)) (field $f1 (mut i64)) (field $f2 (mut i64))))
  (type $user__UserRecord_4 (struct))
  (type $user__UserRecord_5 (struct (field $f0 (mut anyref)) (field $f1 (mut (ref null $rt_types__IterState)))))
  (type $user__UserRecord_8 (struct (field $f0 (mut (ref null $rt_types__IterState))) (field $f1 (mut (ref null $rt_types__Closure)))))
  (type $user__UserRecord_9 (struct (field $f0 (mut (ref null $rt_types__IterState))) (field $f1 (mut (ref null $rt_types__Closure)))))
  (type $user__UserRecord_10 (struct (field $f0 (mut (ref null $rt_types__IterState))) (field $f1 (mut i64))))
  (type $user__option__Byte (struct (field $variant_id i32) (field $payload i32)))
  (type $user__option__String (struct (field $variant_id i32) (field $payload (ref null $rt_types__String))))
  (type $functype_0 (func (param f64) (result (ref $rt_types__String))))
  (type $functype_1 (func (param (ref null $rt_types__String))))
  (type $functype_2 (func (param i32 anyref) (result (ref $rt_types__Array))))
  (type $functype_3 (func (param (ref null $rt_types__Array) i32) (result anyref)))
  (type $functype_4 (func (param (ref null $rt_types__Array) i32 anyref) (result (ref $rt_types__Array))))
  (type $functype_5 (func (param (ref null $rt_types__Array)) (result i32)))
  (type $functype_6 (func (param (ref null $rt_types__Array) (ref null $rt_types__Array)) (result (ref $rt_types__Array))))
  (type $functype_7 (func (param (ref null $rt_types__Array) i32 i32) (result (ref $rt_types__Array))))
  (type $functype_8 (func (result (ref $rt_types__Array))))
  (type $functype_9 (func (param (ref null $rt_types__Array)) (result (ref $rt_types__Array))))
  (type $functype_10 (func (param (ref null $rt_types__Array) anyref)))
  (type $functype_11 (func (param (ref null $rt_types__String)) (result i32)))
  (type $functype_12 (func (param (ref null $rt_types__String) (ref null $rt_types__String)) (result (ref $rt_types__String))))
  (type $functype_13 (func (param (ref null $rt_types__String) i32 i32) (result (ref $rt_types__String))))
  (type $functype_14 (func (param (ref null $rt_types__String) (ref null $rt_types__String)) (result i32)))
  (type $functype_15 (func (param i64) (result (ref $rt_types__String))))
  (type $functype_16 (func (param i32) (result (ref $rt_types__String))))
  (type $functype_17 (func (result (ref $rt_types__Dict))))
  (type $functype_18 (func (param (ref null $rt_types__Dict)) (result i32)))
  (type $functype_19 (func (param (ref null $rt_types__Dict)) (result (ref $rt_types__Array))))
  (type $functype_20 (func (param (ref null $rt_types__Dict) anyref) (result i32)))
  (type $functype_21 (func (param (ref null $rt_types__Dict) anyref) (result anyref)))
  (type $functype_22 (func (param (ref null $rt_types__Dict) anyref) (result (ref $rt_types__Variant))))
  (type $functype_23 (func (param (ref null $rt_types__Dict) anyref anyref) (result (ref $rt_types__Dict))))
  (type $functype_24 (func (param (ref null $rt_types__Dict) anyref) (result (ref $rt_types__Dict))))
  (type $functype_25 (func (param anyref anyref) (result i32)))
  (type $functype_26 (func (param (ref null $rt_types__Variant)) (result (ref null $rt_types__String))))
  (type $functype_27 (func (param i64) (result i32)))
  (type $functype_28 (func))
  (type $functype_29 (func (param anyref anyref) (result anyref)))
  (type $functype_30 (func (param anyref) (result (ref null $rt_types__Variant))))
  (type $functype_31 (func (param (ref null $rt_types__String)) (result anyref)))
  (type $functype_32 (func (param i32) (result anyref)))
  (type $functype_33 (func (param (ref null $rt_types__String)) (result (ref $rt_types__Array))))
  (type $functype_34 (func (param (ref null $rt_types__Array)) (result anyref)))
  (type $functype_35 (func (result (ref $rt_types__String))))
  (import "host" "f64_to_string" (func $rt_str__host_f64_to_string (type $functype_0)))
  (import "host" "print" (func $rt_core__host_print (type $functype_1)))
  (import "host" "println" (func $rt_core__host_println (type $functype_1)))
  (import "host" "error" (func $rt_core__host_error (type $functype_1)))
  (import "host" "eprint" (func $rt_core__host_eprint (type $functype_1)))
  (import "host" "eprintln" (func $rt_core__host_eprintln (type $functype_1)))
  (global $user____str_lit_global_empty (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_29 (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_4641494c (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_4e6f6e65 (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_536f6d6528 (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_6261642062797465 (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_68656c6c6f (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_6e6f6e2d65786861757374697665206d6174636820696e205f5f696e69745f5f202846756e6349642834332929 (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_6e6f6e2d65786861757374697665206d6174636820696e2062797465202846756e6349642834322929 (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_6e6f6e2d65786861757374697665206d6174636820696e2073686f775f6f7074202846756e6349642834312929 (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_726f756e642d747269703a20 (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_78 (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_c3a9 (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_e4bda0e5a5bde4b896e7958c (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_f09f918d (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (func $rt_arr__make (type $functype_2)
    (param $p0 i32)
    (param $p1 anyref)
    (result (ref $rt_types__Array))
    local.get $p1
    local.get $p0
    array.new $rt_types__Array
  )
  (func $rt_arr__get (type $functype_3)
    (param $p0 (ref null $rt_types__Array))
    (param $p1 i32)
    (result anyref)
    local.get $p0
    ref.as_non_null
    local.get $p1
    array.get $rt_types__Array
  )
  (func $rt_arr__set (type $functype_4)
    (param $p0 (ref null $rt_types__Array))
    (param $p1 i32)
    (param $p2 anyref)
    (result (ref $rt_types__Array))
    (local $p3 (ref null $rt_types__Array))
    (local $p4 i32)
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p4
    ref.null none
    local.get $p4
    array.new $rt_types__Array
    local.set $p3
    local.get $p3
    ref.as_non_null
    i32.const 0
    local.get $p0
    ref.as_non_null
    i32.const 0
    local.get $p4
    array.copy $rt_types__Array $rt_types__Array
    local.get $p3
    ref.as_non_null
    local.get $p1
    local.get $p2
    array.set $rt_types__Array
    local.get $p3
    ref.as_non_null
  )
  (func $rt_arr__len (type $functype_5)
    (param $p0 (ref null $rt_types__Array))
    (result i32)
    local.get $p0
    ref.as_non_null
    array.len
  )
  (func $rt_arr__concat (type $functype_6)
    (param $p0 (ref null $rt_types__Array))
    (param $p1 (ref null $rt_types__Array))
    (result (ref $rt_types__Array))
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 (ref null $rt_types__Array))
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p2
    local.get $p1
    ref.as_non_null
    array.len
    local.set $p3
    local.get $p2
    local.get $p3
    i32.add
    local.set $p4
    ref.null none
    local.get $p4
    array.new $rt_types__Array
    local.set $p5
    local.get $p5
    ref.as_non_null
    i32.const 0
    local.get $p0
    ref.as_non_null
    i32.const 0
    local.get $p2
    array.copy $rt_types__Array $rt_types__Array
    local.get $p5
    ref.as_non_null
    local.get $p2
    local.get $p1
    ref.as_non_null
    i32.const 0
    local.get $p3
    array.copy $rt_types__Array $rt_types__Array
    local.get $p5
    ref.as_non_null
  )
  (func $rt_arr__slice (type $functype_7)
    (param $p0 (ref null $rt_types__Array))
    (param $p1 i32)
    (param $p2 i32)
    (result (ref $rt_types__Array))
    (local $p3 i32)
    (local $p4 (ref null $rt_types__Array))
    local.get $p2
    local.get $p1
    i32.sub
    local.set $p3
    ref.null none
    local.get $p3
    array.new $rt_types__Array
    local.set $p4
    local.get $p4
    ref.as_non_null
    i32.const 0
    local.get $p0
    ref.as_non_null
    local.get $p1
    local.get $p3
    array.copy $rt_types__Array $rt_types__Array
    local.get $p4
    ref.as_non_null
  )
  (func $rt_arr__builder_new (type $functype_8)
    (result (ref $rt_types__Array))
    ref.null none
    i32.const 8
    array.new $rt_types__Array
    i64.const 0
    struct.new $rt_types__BoxedInt
    i64.const 8
    struct.new $rt_types__BoxedInt
    array.new_fixed $rt_types__Array 3
  )
  (func $rt_arr__builder_from (type $functype_9)
    (param $p0 (ref null $rt_types__Array))
    (result (ref $rt_types__Array))
    (local $p1 i32)
    (local $p2 i32)
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p1
    local.get $p1
    local.set $p2
    local.get $p0
    ref.as_non_null
    local.get $p1
    i64.extend_i32_s
    struct.new $rt_types__BoxedInt
    local.get $p2
    i64.extend_i32_s
    struct.new $rt_types__BoxedInt
    array.new_fixed $rt_types__Array 3
  )
  (func $rt_arr__builder_push (type $functype_10)
    (param $p0 (ref null $rt_types__Array))
    (param $p1 anyref)
    (local $p2 (ref null $rt_types__Array))
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 i32)
    (local $p6 (ref null $rt_types__Array))
    local.get $p0
    ref.as_non_null
    i32.const 0
    array.get $rt_types__Array
    ref.cast (ref null $rt_types__Array)
    local.set $p2
    local.get $p0
    ref.as_non_null
    i32.const 1
    array.get $rt_types__Array
    ref.cast (ref $rt_types__BoxedInt)
    struct.get $rt_types__BoxedInt 0
    i32.wrap_i64
    local.set $p3
    local.get $p0
    ref.as_non_null
    i32.const 2
    array.get $rt_types__Array
    ref.cast (ref $rt_types__BoxedInt)
    struct.get $rt_types__BoxedInt 0
    i32.wrap_i64
    local.set $p4
    local.get $p3
    local.get $p4
    i32.lt_s
    (if
      (then
        local.get $p2
        ref.as_non_null
        local.get $p3
        local.get $p1
        array.set $rt_types__Array
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        local.get $p0
        ref.as_non_null
        i32.const 1
        local.get $p3
        i64.extend_i32_s
        struct.new $rt_types__BoxedInt
        array.set $rt_types__Array)
      (else
        local.get $p4
        i32.eqz
        (if (result i32)
          (then
            i32.const 8)
          (else
            local.get $p4
            i32.const 2
            i32.mul))
        local.set $p5
        ref.null none
        local.get $p5
        array.new $rt_types__Array
        local.set $p6
        local.get $p6
        ref.as_non_null
        i32.const 0
        local.get $p2
        ref.as_non_null
        i32.const 0
        local.get $p3
        array.copy $rt_types__Array $rt_types__Array
        local.get $p6
        ref.as_non_null
        local.get $p3
        local.get $p1
        array.set $rt_types__Array
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        local.get $p5
        local.set $p4
        local.get $p0
        ref.as_non_null
        i32.const 0
        local.get $p6
        array.set $rt_types__Array
        local.get $p0
        ref.as_non_null
        i32.const 1
        local.get $p3
        i64.extend_i32_s
        struct.new $rt_types__BoxedInt
        array.set $rt_types__Array
        local.get $p0
        ref.as_non_null
        i32.const 2
        local.get $p4
        i64.extend_i32_s
        struct.new $rt_types__BoxedInt
        array.set $rt_types__Array))
  )
  (func $rt_arr__builder_freeze (type $functype_9)
    (param $p0 (ref null $rt_types__Array))
    (result (ref $rt_types__Array))
    (local $p1 (ref null $rt_types__Array))
    (local $p2 i32)
    (local $p3 (ref null $rt_types__Array))
    local.get $p0
    ref.as_non_null
    i32.const 0
    array.get $rt_types__Array
    ref.cast (ref null $rt_types__Array)
    local.set $p1
    local.get $p0
    ref.as_non_null
    i32.const 1
    array.get $rt_types__Array
    ref.cast (ref $rt_types__BoxedInt)
    struct.get $rt_types__BoxedInt 0
    i32.wrap_i64
    local.set $p2
    ref.null none
    local.get $p2
    array.new $rt_types__Array
    local.set $p3
    local.get $p3
    ref.as_non_null
    i32.const 0
    local.get $p1
    ref.as_non_null
    i32.const 0
    local.get $p2
    array.copy $rt_types__Array $rt_types__Array
    local.get $p3
    ref.as_non_null
  )
  (func $rt_str__len (type $functype_11)
    (param $p0 (ref null $rt_types__String))
    (result i32)
    local.get $p0
    ref.as_non_null
    array.len
  )
  (func $rt_str__concat (type $functype_12)
    (param $p0 (ref null $rt_types__String))
    (param $p1 (ref null $rt_types__String))
    (result (ref $rt_types__String))
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 (ref null $rt_types__String))
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p2
    local.get $p1
    ref.as_non_null
    array.len
    local.set $p3
    local.get $p2
    local.get $p3
    i32.add
    local.set $p4
    i32.const 0
    local.get $p4
    array.new $rt_types__String
    local.set $p5
    local.get $p5
    ref.as_non_null
    i32.const 0
    local.get $p0
    ref.as_non_null
    i32.const 0
    local.get $p2
    array.copy $rt_types__String $rt_types__String
    local.get $p5
    ref.as_non_null
    local.get $p2
    local.get $p1
    ref.as_non_null
    i32.const 0
    local.get $p3
    array.copy $rt_types__String $rt_types__String
    local.get $p5
    ref.as_non_null
  )
  (func $rt_str__substring (type $functype_13)
    (param $p0 (ref null $rt_types__String))
    (param $p1 i32)
    (param $p2 i32)
    (result (ref $rt_types__String))
    (local $p3 i32)
    (local $p4 (ref null $rt_types__String))
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p3
    local.get $p1
    i32.const 0
    i32.lt_s
    (if
      (then
        local.get $p3
        local.set $p1))
    local.get $p1
    local.get $p3
    i32.gt_s
    (if
      (then
        local.get $p3
        local.set $p1))
    local.get $p2
    i32.const 0
    i32.lt_s
    (if
      (then
        local.get $p3
        local.set $p2))
    local.get $p2
    local.get $p3
    i32.gt_s
    (if
      (then
        local.get $p3
        local.set $p2))
    local.get $p2
    local.get $p1
    i32.lt_s
    (if
      (then
        local.get $p1
        local.set $p2))
    local.get $p2
    local.get $p1
    i32.sub
    local.set $p3
    i32.const 0
    local.get $p3
    array.new $rt_types__String
    local.set $p4
    local.get $p4
    ref.as_non_null
    i32.const 0
    local.get $p0
    ref.as_non_null
    local.get $p1
    local.get $p3
    array.copy $rt_types__String $rt_types__String
    local.get $p4
    ref.as_non_null
  )
  (func $rt_str__eq (type $functype_14)
    (param $p0 (ref null $rt_types__String))
    (param $p1 (ref null $rt_types__String))
    (result i32)
    (local $p2 i32)
    (local $p3 i32)
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p2
    local.get $p2
    local.get $p1
    ref.as_non_null
    array.len
    i32.ne
    (if
      (then
        i32.const 0
        return))
    i32.const 0
    local.set $p3
    (block $exit
      (loop $cmp
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $exit
        local.get $p0
        ref.as_non_null
        local.get $p3
        array.get_u $rt_types__String
        local.get $p1
        ref.as_non_null
        local.get $p3
        array.get_u $rt_types__String
        i32.ne
        (if
          (then
            i32.const 0
            return))
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $cmp))
    i32.const 1
  )
  (func $rt_str__cmp (type $functype_14)
    (param $p0 (ref null $rt_types__String))
    (param $p1 (ref null $rt_types__String))
    (result i32)
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 i32)
    (local $p6 i32)
    (local $p7 i32)
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p6
    local.get $p1
    ref.as_non_null
    array.len
    local.set $p7
    local.get $p6
    local.get $p7
    local.get $p6
    local.get $p7
    i32.le_s
    select
    local.set $p2
    i32.const 0
    local.set $p3
    (block $done
      (loop $cmp_loop
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $done
        local.get $p0
        ref.as_non_null
        local.get $p3
        array.get_u $rt_types__String
        local.set $p4
        local.get $p1
        ref.as_non_null
        local.get $p3
        array.get_u $rt_types__String
        local.set $p5
        local.get $p4
        local.get $p5
        i32.lt_u
        (if
          (then
            i32.const -1
            return))
        local.get $p4
        local.get $p5
        i32.gt_u
        (if
          (then
            i32.const 1
            return))
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $cmp_loop))
    local.get $p6
    local.get $p7
    i32.lt_s
    (if
      (then
        i32.const -1
        return))
    local.get $p6
    local.get $p7
    i32.gt_s
    (if
      (then
        i32.const 1
        return))
    i32.const 0
  )
  (func $rt_str__from_i64 (type $functype_15)
    (param $p0 i64)
    (result (ref $rt_types__String))
    (local $p1 i32)
    (local $p2 i64)
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 (ref null $rt_types__String))
    (local $p6 (ref null $rt_types__String))
    local.get $p0
    i64.eqz
    (if
      (then
        i32.const 48
        array.new_fixed $rt_types__String 1
        return))
    local.get $p0
    i64.const -9223372036854775808
    i64.eq
    (if
      (then
        i32.const 45
        i32.const 57
        i32.const 50
        i32.const 50
        i32.const 51
        i32.const 51
        i32.const 55
        i32.const 50
        i32.const 48
        i32.const 51
        i32.const 54
        i32.const 56
        i32.const 53
        i32.const 52
        i32.const 55
        i32.const 55
        i32.const 53
        i32.const 56
        i32.const 48
        i32.const 56
        array.new_fixed $rt_types__String 20
        return))
    local.get $p0
    i64.const 0
    i64.lt_s
    local.set $p1
    local.get $p1
    (if (result i64)
      (then
        i64.const 0
        local.get $p0
        i64.sub)
      (else
        local.get $p0))
    local.set $p2
    i32.const 0
    i32.const 20
    array.new $rt_types__String
    local.set $p5
    i32.const 19
    local.set $p3
    (loop $digits
      local.get $p5
      ref.as_non_null
      local.get $p3
      local.get $p2
      i64.const 10
      i64.rem_s
      i32.wrap_i64
      i32.const 48
      i32.add
      array.set $rt_types__String
      local.get $p2
      i64.const 10
      i64.div_s
      local.set $p2
      local.get $p3
      i32.const 1
      i32.sub
      local.set $p3
      local.get $p2
      i64.eqz
      i32.eqz
      br_if $digits)
    local.get $p1
    (if
      (then
        local.get $p5
        ref.as_non_null
        local.get $p3
        i32.const 45
        array.set $rt_types__String
        local.get $p3
        i32.const 1
        i32.sub
        local.set $p3))
    i32.const 19
    local.get $p3
    i32.sub
    local.set $p4
    i32.const 0
    local.get $p4
    array.new $rt_types__String
    local.set $p6
    local.get $p6
    ref.as_non_null
    i32.const 0
    local.get $p5
    ref.as_non_null
    local.get $p3
    i32.const 1
    i32.add
    local.get $p4
    array.copy $rt_types__String $rt_types__String
    local.get $p6
    ref.as_non_null
  )
  (func $rt_str__from_f64 (type $functype_0)
    (param $p0 f64)
    (result (ref $rt_types__String))
    local.get $p0
    call $rt_str__host_f64_to_string
  )
  (func $rt_str__from_bool (type $functype_16)
    (param $p0 i32)
    (result (ref $rt_types__String))
    local.get $p0
    (if (result (ref $rt_types__String))
      (then
        i32.const 116
        i32.const 114
        i32.const 117
        i32.const 101
        array.new_fixed $rt_types__String 4)
      (else
        i32.const 102
        i32.const 97
        i32.const 108
        i32.const 115
        i32.const 101
        array.new_fixed $rt_types__String 5))
  )
  (func $rt_dict__make (type $functype_17)
    (result (ref $rt_types__Dict))
    array.new_fixed $rt_types__Dict 0
  )
  (func $rt_dict__len (type $functype_18)
    (param $p0 (ref null $rt_types__Dict))
    (result i32)
    local.get $p0
    ref.as_non_null
    array.len
  )
  (func $rt_dict__keys (type $functype_19)
    (param $p0 (ref null $rt_types__Dict))
    (result (ref $rt_types__Array))
    (local $p1 i32)
    (local $p2 i32)
    (local $p3 (ref null $rt_types__Array))
    (local $p4 (ref null $rt_types__DictEntry))
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p1
    ref.null none
    local.get $p1
    array.new $rt_types__Array
    local.set $p3
    i32.const 0
    local.set $p2
    (block $exit
      (loop $loop
        local.get $p2
        local.get $p1
        i32.ge_s
        br_if $exit
        local.get $p0
        ref.as_non_null
        local.get $p2
        array.get $rt_types__Dict
        local.set $p4
        local.get $p3
        ref.as_non_null
        local.get $p2
        local.get $p4
        ref.as_non_null
        struct.get $rt_types__DictEntry 0
        array.set $rt_types__Array
        local.get $p2
        i32.const 1
        i32.add
        local.set $p2
        br $loop))
    local.get $p3
    ref.as_non_null
  )
  (func $rt_dict__has (type $functype_20)
    (param $p0 (ref null $rt_types__Dict))
    (param $p1 anyref)
    (result i32)
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 (ref null $rt_types__DictEntry))
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p2
    i32.const 0
    local.set $p3
    (block $exit
      (loop $scan
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $exit
        local.get $p0
        ref.as_non_null
        local.get $p3
        array.get $rt_types__Dict
        local.set $p4
        local.get $p4
        ref.as_non_null
        struct.get $rt_types__DictEntry 0
        local.get $p1
        call $rt_core__eq
        (if
          (then
            i32.const 1
            return))
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $scan))
    i32.const 0
  )
  (func $rt_dict__get (type $functype_21)
    (param $p0 (ref null $rt_types__Dict))
    (param $p1 anyref)
    (result anyref)
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 (ref null $rt_types__DictEntry))
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p2
    i32.const 0
    local.set $p3
    (block $exit
      (loop $scan
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $exit
        local.get $p0
        ref.as_non_null
        local.get $p3
        array.get $rt_types__Dict
        local.set $p4
        local.get $p4
        ref.as_non_null
        struct.get $rt_types__DictEntry 0
        local.get $p1
        call $rt_core__eq
        (if
          (then
            local.get $p4
            ref.as_non_null
            struct.get $rt_types__DictEntry 1
            return))
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $scan))
    ref.null any
  )
  (func $rt_dict__get_option (type $functype_22)
    (param $p0 (ref null $rt_types__Dict))
    (param $p1 anyref)
    (result (ref $rt_types__Variant))
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 (ref null $rt_types__DictEntry))
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p2
    i32.const 0
    local.set $p3
    (block $exit
      (loop $scan
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $exit
        local.get $p0
        ref.as_non_null
        local.get $p3
        array.get $rt_types__Dict
        local.set $p4
        local.get $p4
        ref.as_non_null
        struct.get $rt_types__DictEntry 0
        local.get $p1
        call $rt_core__eq
        (if
          (then
            i32.const 0
            i32.const 1
            local.get $p4
            ref.as_non_null
            struct.get $rt_types__DictEntry 1
            array.new_fixed $rt_types__Array 1
            struct.new $rt_types__Variant
            return))
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $scan))
    i32.const 0
    i32.const 0
    ref.null $rt_types__Array
    struct.new $rt_types__Variant
  )
  (func $rt_dict__set (type $functype_23)
    (param $p0 (ref null $rt_types__Dict))
    (param $p1 anyref)
    (param $p2 anyref)
    (result (ref $rt_types__Dict))
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 i32)
    (local $p6 (ref null $rt_types__Dict))
    (local $p7 (ref $rt_types__DictEntry))
    (local $p8 (ref null $rt_types__DictEntry))
    (local $p9 i32)
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p3
    local.get $p1
    local.get $p2
    struct.new $rt_types__DictEntry
    local.set $p7
    i32.const 0
    local.set $p4
    i32.const 0
    local.set $p5
    (block $found_exit
      (loop $scan
        local.get $p4
        local.get $p3
        i32.ge_s
        br_if $found_exit
        local.get $p0
        ref.as_non_null
        local.get $p4
        array.get $rt_types__Dict
        local.set $p8
        local.get $p8
        ref.as_non_null
        struct.get $rt_types__DictEntry 0
        local.get $p1
        call $rt_core__eq
        (if
          (then
            i32.const 1
            local.set $p5
            br $found_exit))
        local.get $p4
        i32.const 1
        i32.add
        local.set $p4
        br $scan))
    local.get $p5
    (if (result i32)
      (then
        local.get $p3)
      (else
        local.get $p3
        i32.const 1
        i32.add))
    local.set $p9
    ref.null $rt_types__DictEntry
    local.get $p9
    array.new $rt_types__Dict
    local.set $p6
    i32.const 0
    local.set $p4
    (block $copy_exit
      (loop $copy
        local.get $p4
        local.get $p3
        i32.ge_s
        br_if $copy_exit
        local.get $p0
        ref.as_non_null
        local.get $p4
        array.get $rt_types__Dict
        local.set $p8
        local.get $p8
        ref.as_non_null
        struct.get $rt_types__DictEntry 0
        local.get $p1
        call $rt_core__eq
        (if
          (then
            local.get $p6
            ref.as_non_null
            local.get $p4
            local.get $p7
            array.set $rt_types__Dict)
          (else
            local.get $p6
            ref.as_non_null
            local.get $p4
            local.get $p8
            array.set $rt_types__Dict))
        local.get $p4
        i32.const 1
        i32.add
        local.set $p4
        br $copy))
    local.get $p5
    i32.eqz
    (if
      (then
        local.get $p6
        ref.as_non_null
        local.get $p3
        local.get $p7
        array.set $rt_types__Dict))
    local.get $p6
    ref.as_non_null
  )
  (func $rt_dict__remove (type $functype_24)
    (param $p0 (ref null $rt_types__Dict))
    (param $p1 anyref)
    (result (ref $rt_types__Dict))
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 (ref null $rt_types__Dict))
    (local $p6 (ref null $rt_types__DictEntry))
    (local $p7 i32)
    (local $p8 i32)
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p2
    i32.const 0
    local.set $p3
    i32.const 0
    local.set $p7
    (block $scan_exit
      (loop $scan
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $scan_exit
        local.get $p0
        ref.as_non_null
        local.get $p3
        array.get $rt_types__Dict
        local.set $p6
        local.get $p6
        ref.as_non_null
        struct.get $rt_types__DictEntry 0
        local.get $p1
        call $rt_core__eq
        (if
          (then
            i32.const 1
            local.set $p7
            br $scan_exit))
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $scan))
    local.get $p7
    (if (result i32)
      (then
        local.get $p2
        i32.const 1
        i32.sub)
      (else
        local.get $p2))
    local.set $p8
    ref.null $rt_types__DictEntry
    local.get $p8
    array.new $rt_types__Dict
    local.set $p5
    i32.const 0
    local.set $p3
    i32.const 0
    local.set $p4
    (block $copy_exit
      (loop $copy
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $copy_exit
        local.get $p0
        ref.as_non_null
        local.get $p3
        array.get $rt_types__Dict
        local.set $p6
        local.get $p6
        ref.as_non_null
        struct.get $rt_types__DictEntry 0
        local.get $p1
        call $rt_core__eq
        (if
          (then)
          (else
            local.get $p5
            ref.as_non_null
            local.get $p4
            local.get $p6
            array.set $rt_types__Dict
            local.get $p4
            i32.const 1
            i32.add
            local.set $p4))
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $copy))
    local.get $p5
    ref.as_non_null
  )
  (func $rt_dict__set_in_place (type $functype_23)
    (param $p0 (ref null $rt_types__Dict))
    (param $p1 anyref)
    (param $p2 anyref)
    (result (ref $rt_types__Dict))
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 (ref null $rt_types__DictEntry))
    (local $p6 (ref null $rt_types__Dict))
    (local $p7 (ref $rt_types__DictEntry))
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p3
    local.get $p1
    local.get $p2
    struct.new $rt_types__DictEntry
    local.set $p7
    i32.const 0
    local.set $p4
    (block $not_found
      (loop $scan
        local.get $p4
        local.get $p3
        i32.ge_s
        br_if $not_found
        local.get $p0
        ref.as_non_null
        local.get $p4
        array.get $rt_types__Dict
        local.set $p5
        local.get $p5
        ref.as_non_null
        struct.get $rt_types__DictEntry 0
        local.get $p1
        call $rt_core__eq
        (if
          (then
            local.get $p0
            ref.as_non_null
            local.get $p4
            local.get $p7
            array.set $rt_types__Dict
            local.get $p0
            ref.as_non_null
            return))
        local.get $p4
        i32.const 1
        i32.add
        local.set $p4
        br $scan))
    ref.null $rt_types__DictEntry
    local.get $p3
    i32.const 1
    i32.add
    array.new $rt_types__Dict
    local.set $p6
    local.get $p6
    ref.as_non_null
    i32.const 0
    local.get $p0
    ref.as_non_null
    i32.const 0
    local.get $p3
    array.copy $rt_types__Dict $rt_types__Dict
    local.get $p6
    ref.as_non_null
    local.get $p3
    local.get $p7
    array.set $rt_types__Dict
    local.get $p6
    ref.as_non_null
  )
  (func $rt_dict__remove_in_place (type $functype_24)
    (param $p0 (ref null $rt_types__Dict))
    (param $p1 anyref)
    (result (ref $rt_types__Dict))
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 (ref null $rt_types__DictEntry))
    (local $p6 (ref null $rt_types__Dict))
    (local $p7 i32)
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p2
    i32.const 0
    local.set $p3
    i32.const -1
    local.set $p4
    (block $scan_exit
      (loop $scan
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $scan_exit
        local.get $p0
        ref.as_non_null
        local.get $p3
        array.get $rt_types__Dict
        local.set $p5
        local.get $p5
        ref.as_non_null
        struct.get $rt_types__DictEntry 0
        local.get $p1
        call $rt_core__eq
        (if
          (then
            local.get $p3
            local.set $p4
            br $scan_exit))
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $scan))
    local.get $p4
    i32.const 1
    i32.add
    i32.eqz
    (if
      (then
        local.get $p0
        ref.as_non_null
        return))
    ref.null $rt_types__DictEntry
    local.get $p2
    i32.const 1
    i32.sub
    array.new $rt_types__Dict
    local.set $p6
    local.get $p4
    i32.const 0
    i32.gt_s
    (if
      (then
        local.get $p6
        ref.as_non_null
        i32.const 0
        local.get $p0
        ref.as_non_null
        i32.const 0
        local.get $p4
        array.copy $rt_types__Dict $rt_types__Dict))
    local.get $p2
    local.get $p4
    i32.sub
    i32.const 1
    i32.sub
    local.set $p7
    local.get $p7
    i32.const 0
    i32.gt_s
    (if
      (then
        local.get $p6
        ref.as_non_null
        local.get $p4
        local.get $p0
        ref.as_non_null
        local.get $p4
        i32.const 1
        i32.add
        local.get $p7
        array.copy $rt_types__Dict $rt_types__Dict))
    local.get $p6
    ref.as_non_null
  )
  (func $rt_core__print (type $functype_1)
    (param $p0 (ref null $rt_types__String))
    local.get $p0
    call $rt_core__host_print
  )
  (func $rt_core__println (type $functype_1)
    (param $p0 (ref null $rt_types__String))
    local.get $p0
    call $rt_core__host_println
  )
  (func $rt_core__eprint (type $functype_1)
    (param $p0 (ref null $rt_types__String))
    local.get $p0
    call $rt_core__host_eprint
  )
  (func $rt_core__eprintln (type $functype_1)
    (param $p0 (ref null $rt_types__String))
    local.get $p0
    call $rt_core__host_eprintln
  )
  (func $rt_core__trap (type $functype_1)
    (param $p0 (ref null $rt_types__String))
    local.get $p0
    call $rt_core__host_error
    unreachable
  )
  (func $rt_core__eq (type $functype_25)
    (param $p0 anyref)
    (param $p1 anyref)
    (result i32)
    local.get $p0
    ref.cast (ref null eq)
    local.get $p1
    ref.cast (ref null eq)
    ref.eq
    (if
      (then
        i32.const 1
        return))
    local.get $p0
    ref.test (ref $rt_types__BoxedInt)
    local.get $p1
    ref.test (ref $rt_types__BoxedInt)
    i32.and
    (if
      (then
        local.get $p0
        ref.cast (ref $rt_types__BoxedInt)
        struct.get $rt_types__BoxedInt 0
        local.get $p1
        ref.cast (ref $rt_types__BoxedInt)
        struct.get $rt_types__BoxedInt 0
        i64.eq
        return))
    local.get $p0
    ref.test (ref $rt_types__String)
    local.get $p1
    ref.test (ref $rt_types__String)
    i32.and
    (if
      (then
        local.get $p0
        ref.cast (ref $rt_types__String)
        local.get $p1
        ref.cast (ref $rt_types__String)
        call $rt_str__eq
        return))
    local.get $p0
    ref.cast (ref null eq)
    local.get $p1
    ref.cast (ref null eq)
    ref.eq
  )
  (func $user__func_41 (type $functype_26)
    (param $p0 (ref null $rt_types__Variant))
    (result (ref null $rt_types__String))
    (local $p1 (ref null $rt_types__String))
    (local $p2 (ref null $rt_types__String))
    (local $p3 (ref $rt_types__String))
    (local $p4 (ref $rt_types__String))
    (local $p5 (ref null $rt_types__String))
    local.get $p0
    ref.cast (ref null $rt_types__Variant)
    struct.get $rt_types__Variant 0
    i32.const 0
    i32.eq
    local.get $p0
    ref.cast (ref null $rt_types__Variant)
    struct.get $rt_types__Variant 1
    i32.const 1
    i32.eq
    i32.and
    (if (result (ref null $rt_types__String))
      (then
        local.get $p0
        ref.cast (ref null $rt_types__Variant)
        struct.get $rt_types__Variant 2
        i32.const 0
        array.get $rt_types__Array
        ref.cast (ref null $rt_types__String)
        local.set $p1
        local.get $p1
        local.set $p2
        local.get $p2
        call $user____str_lit_get_29
        call $rt_str__concat
        local.set $p3
        call $user____str_lit_get_536f6d6528
        local.get $p3
        call $rt_str__concat
        local.set $p4
        local.get $p4)
      (else
        local.get $p0
        ref.cast (ref null $rt_types__Variant)
        struct.get $rt_types__Variant 0
        i32.const 0
        i32.eq
        local.get $p0
        ref.cast (ref null $rt_types__Variant)
        struct.get $rt_types__Variant 1
        i32.const 0
        i32.eq
        i32.and
        (if (result (ref null $rt_types__String))
          (then
            call $user____str_lit_get_4e6f6e65)
          (else
            call $user____str_lit_get_6e6f6e2d65786861757374697665206d6174636820696e2073686f775f6f7074202846756e6349642834312929
            call $rt_core__trap
            unreachable
            unreachable))))
    local.set $p5
    local.get $p5
    return
  )
  (func $user__func_42 (type $functype_27)
    (param $p0 i64)
    (result i32)
    (local $p1 anyref)
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 i32)
    local.get $p0
    i64.const 0
    i64.ge_s
    local.get $p0
    i64.const 256
    i64.lt_s
    i32.and
    (if (result anyref)
      (then
        i32.const 0
        i32.const 1
        local.get $p0
        i32.wrap_i64
        ref.i31
        array.new_fixed $rt_types__Array 1
        struct.new $rt_types__Variant)
      (else
        i32.const 0
        i32.const 0
        array.new_fixed $rt_types__Array 0
        struct.new $rt_types__Variant))
    local.set $p1
    local.get $p1
    ref.test (ref null $rt_types__Variant)
    (if (result (ref null $rt_types__Variant))
      (then
        local.get $p1
        ref.cast (ref null $rt_types__Variant))
      (else
        i32.const 0
        local.get $p1
        ref.cast (ref null $user__option__Byte)
        struct.get $user__option__Byte 0
        local.get $p1
        ref.cast (ref null $user__option__Byte)
        struct.get $user__option__Byte 0
        i32.const 1
        i32.eq
        (if (result (ref $rt_types__Array))
          (then
            local.get $p1
            ref.cast (ref null $user__option__Byte)
            struct.get $user__option__Byte 1
            ref.i31
            array.new_fixed $rt_types__Array 1)
          (else
            array.new_fixed $rt_types__Array 0))
        struct.new $rt_types__Variant))
    ref.cast (ref null $rt_types__Variant)
    struct.get $rt_types__Variant 0
    i32.const 0
    i32.eq
    local.get $p1
    ref.test (ref null $rt_types__Variant)
    (if (result (ref null $rt_types__Variant))
      (then
        local.get $p1
        ref.cast (ref null $rt_types__Variant))
      (else
        i32.const 0
        local.get $p1
        ref.cast (ref null $user__option__Byte)
        struct.get $user__option__Byte 0
        local.get $p1
        ref.cast (ref null $user__option__Byte)
        struct.get $user__option__Byte 0
        i32.const 1
        i32.eq
        (if (result (ref $rt_types__Array))
          (then
            local.get $p1
            ref.cast (ref null $user__option__Byte)
            struct.get $user__option__Byte 1
            ref.i31
            array.new_fixed $rt_types__Array 1)
          (else
            array.new_fixed $rt_types__Array 0))
        struct.new $rt_types__Variant))
    ref.cast (ref null $rt_types__Variant)
    struct.get $rt_types__Variant 1
    i32.const 1
    i32.eq
    i32.and
    (if (result i32)
      (then
        local.get $p1
        ref.test (ref null $rt_types__Variant)
        (if (result (ref null $rt_types__Variant))
          (then
            local.get $p1
            ref.cast (ref null $rt_types__Variant))
          (else
            i32.const 0
            local.get $p1
            ref.cast (ref null $user__option__Byte)
            struct.get $user__option__Byte 0
            local.get $p1
            ref.cast (ref null $user__option__Byte)
            struct.get $user__option__Byte 0
            i32.const 1
            i32.eq
            (if (result (ref $rt_types__Array))
              (then
                local.get $p1
                ref.cast (ref null $user__option__Byte)
                struct.get $user__option__Byte 1
                ref.i31
                array.new_fixed $rt_types__Array 1)
              (else
                array.new_fixed $rt_types__Array 0))
            struct.new $rt_types__Variant))
        ref.cast (ref null $rt_types__Variant)
        struct.get $rt_types__Variant 2
        i32.const 0
        array.get $rt_types__Array
        ref.cast (ref i31)
        i31.get_s
        local.set $p2
        local.get $p2)
      (else
        local.get $p1
        ref.test (ref null $rt_types__Variant)
        (if (result (ref null $rt_types__Variant))
          (then
            local.get $p1
            ref.cast (ref null $rt_types__Variant))
          (else
            i32.const 0
            local.get $p1
            ref.cast (ref null $user__option__Byte)
            struct.get $user__option__Byte 0
            local.get $p1
            ref.cast (ref null $user__option__Byte)
            struct.get $user__option__Byte 0
            i32.const 1
            i32.eq
            (if (result (ref $rt_types__Array))
              (then
                local.get $p1
                ref.cast (ref null $user__option__Byte)
                struct.get $user__option__Byte 1
                ref.i31
                array.new_fixed $rt_types__Array 1)
              (else
                array.new_fixed $rt_types__Array 0))
            struct.new $rt_types__Variant))
        ref.cast (ref null $rt_types__Variant)
        struct.get $rt_types__Variant 0
        i32.const 0
        i32.eq
        local.get $p1
        ref.test (ref null $rt_types__Variant)
        (if (result (ref null $rt_types__Variant))
          (then
            local.get $p1
            ref.cast (ref null $rt_types__Variant))
          (else
            i32.const 0
            local.get $p1
            ref.cast (ref null $user__option__Byte)
            struct.get $user__option__Byte 0
            local.get $p1
            ref.cast (ref null $user__option__Byte)
            struct.get $user__option__Byte 0
            i32.const 1
            i32.eq
            (if (result (ref $rt_types__Array))
              (then
                local.get $p1
                ref.cast (ref null $user__option__Byte)
                struct.get $user__option__Byte 1
                ref.i31
                array.new_fixed $rt_types__Array 1)
              (else
                array.new_fixed $rt_types__Array 0))
            struct.new $rt_types__Variant))
        ref.cast (ref null $rt_types__Variant)
        struct.get $rt_types__Variant 1
        i32.const 0
        i32.eq
        i32.and
        (if (result i32)
          (then
            call $user____str_lit_get_6261642062797465
            call $rt_core__println
            i32.const 0
            local.set $p3
            i64.const 0
            i64.const 0
            i64.ge_s
            i64.const 0
            call $user____str_lit_get_78
            array.len
            i64.extend_i32_u
            i64.lt_s
            i32.and
            (if
              (then)
              (else
                unreachable))
            call $user____str_lit_get_78
            ref.as_non_null
            i64.const 0
            i32.wrap_i64
            array.get_u $rt_types__String
            local.set $p4
            local.get $p4)
          (else
            call $user____str_lit_get_6e6f6e2d65786861757374697665206d6174636820696e2062797465202846756e6349642834322929
            call $rt_core__trap
            unreachable
            unreachable))))
    local.set $p5
    local.get $p5
    return
  )
  (func $user__func_43 (type $functype_28)
    (local $p0 anyref)
    (local $p1 (ref null $rt_types__Array))
    (local $p2 i64)
    (local $p3 (ref $rt_types__String))
    (local $p4 i32)
    (local $p5 (ref null $rt_types__Array))
    (local $p6 i64)
    (local $p7 i64)
    (local $p8 i64)
    (local $p9 i32)
    (local $p10 i32)
    (local $p11 i32)
    (local $p12 i64)
    (local $p13 i32)
    (local $p14 i64)
    (local $p15 (ref $rt_types__String))
    (local $p16 i32)
    (local $p17 i32)
    (local $p18 i32)
    (local $p19 anyref)
    (local $p20 (ref null $rt_types__String))
    (local $p21 i32)
    (local $p22 anyref)
    (local $p23 (ref null $rt_types__Array))
    (local $p24 i64)
    (local $p25 (ref $rt_types__String))
    (local $p26 i32)
    (local $p27 (ref null $rt_types__Array))
    (local $p28 i64)
    (local $p29 i64)
    (local $p30 i64)
    (local $p31 i32)
    (local $p32 i32)
    (local $p33 i32)
    (local $p34 i64)
    (local $p35 i32)
    (local $p36 i64)
    (local $p37 (ref $rt_types__String))
    (local $p38 i32)
    (local $p39 i32)
    (local $p40 i32)
    (local $p41 anyref)
    (local $p42 (ref null $rt_types__String))
    (local $p43 i32)
    (local $p44 anyref)
    (local $p45 (ref null $rt_types__Array))
    (local $p46 i64)
    (local $p47 (ref $rt_types__String))
    (local $p48 i32)
    (local $p49 (ref null $rt_types__Array))
    (local $p50 i64)
    (local $p51 i64)
    (local $p52 i64)
    (local $p53 i32)
    (local $p54 i32)
    (local $p55 i32)
    (local $p56 i64)
    (local $p57 i32)
    (local $p58 i64)
    (local $p59 (ref $rt_types__String))
    (local $p60 i32)
    (local $p61 i32)
    (local $p62 i32)
    (local $p63 anyref)
    (local $p64 (ref null $rt_types__String))
    (local $p65 i32)
    (local $p66 anyref)
    (local $p67 (ref null $rt_types__Array))
    (local $p68 i64)
    (local $p69 (ref $rt_types__String))
    (local $p70 i32)
    (local $p71 anyref)
    (local $p72 (ref null $rt_types__String))
    (local $p73 i32)
    (local $p74 i32)
    (local $p75 (ref null $rt_types__Array))
    (local $p76 (ref null $rt_types__Array))
    (local $p77 anyref)
    (local $p78 (ref null $rt_types__String))
    (local $p79 i32)
    (local $p80 i32)
    (local $p81 i32)
    (local $p82 (ref null $rt_types__Array))
    (local $p83 (ref null $rt_types__Array))
    (local $p84 anyref)
    (local $p85 (ref null $rt_types__String))
    (local $p86 i32)
    (local $p87 i32)
    (local $p88 (ref null $rt_types__Array))
    (local $p89 (ref null $rt_types__Array))
    (local $p90 anyref)
    (local $p91 (ref null $rt_types__String))
    (local $p92 i32)
    (local $p93 i32)
    (local $p94 i32)
    (local $p95 i32)
    (local $p96 (ref null $rt_types__Array))
    (local $p97 (ref null $rt_types__Array))
    (local $p98 anyref)
    (local $p99 (ref null $rt_types__String))
    (local $p100 i32)
    (local $p101 anyref)
    (local $p102 anyref)
    (local $p103 (ref null $rt_types__String))
    (local $p104 (ref null $rt_types__String))
    (local $p105 (ref $rt_types__String))
    (local $p106 i32)
    (local $p107 i32)
    (local $p108 i32)
    (local $p109 anyref)
    (local $p110 anyref)
    (local $p111 (ref null $rt_types__String))
    (local $p112 (ref null $rt_types__String))
    (local $p113 (ref $rt_types__String))
    (local $p114 i32)
    (local $p115 i32)
    (local $p116 i32)
    call $user____str_lit_get_68656c6c6f
    call $user__$string_utf8_bytes_helper
    local.set $p0
    local.get $p0
    ref.cast (ref null $rt_types__Array)
    local.set $p1
    local.get $p1
    call $rt_arr__len
    i64.extend_i32_s
    local.set $p2
    local.get $p2
    call $rt_str__from_i64
    local.set $p3
    local.get $p3
    call $rt_core__println
    i32.const 0
    local.set $p4
    local.get $p1
    local.set $p5
    local.get $p5
    call $rt_arr__len
    i64.extend_i32_s
    local.set $p6
    local.get $p6
    local.set $p7
    i64.const 0
    local.set $p8
    (block $break_0 (result i32)
      (loop $cont_0
        local.get $p8
        local.get $p7
        i64.ge_s
        local.set $p9
        local.get $p9
        (if
          (then
            i32.const 0
            br $break_0)
          (else
            local.get $p5
            local.get $p8
            i32.wrap_i64
            call $rt_arr__get
            ref.cast (ref i31)
            i31.get_s
            local.set $p10
            local.get $p10
            local.set $p11
            local.get $p8
            i64.const 1
            i64.add
            local.set $p12
            local.get $p12
            local.set $p8
            i32.const 0
            local.set $p13
            local.get $p11
            i64.extend_i32_u
            local.set $p14
            local.get $p14
            call $rt_str__from_i64
            local.set $p15
            local.get $p15
            call $rt_core__println
            i32.const 0
            local.set $p16
            br $cont_0))
        local.get $p17
        drop
        br $cont_0)
      unreachable)
    local.set $p18
    local.get $p1
    call $user__$string_from_utf8_helper
    local.set $p19
    local.get $p19
    ref.test (ref null $rt_types__Variant)
    (if (result (ref null $rt_types__Variant))
      (then
        local.get $p19
        ref.cast (ref null $rt_types__Variant))
      (else
        i32.const 0
        local.get $p19
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        local.get $p19
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        i32.const 1
        i32.eq
        (if (result (ref $rt_types__Array))
          (then
            local.get $p19
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 1
            array.new_fixed $rt_types__Array 1)
          (else
            array.new_fixed $rt_types__Array 0))
        struct.new $rt_types__Variant))
    call $user__func_41
    local.set $p20
    local.get $p20
    call $rt_core__println
    i32.const 0
    local.set $p21
    call $user____str_lit_get_c3a9
    call $user__$string_utf8_bytes_helper
    local.set $p22
    local.get $p22
    ref.cast (ref null $rt_types__Array)
    local.set $p23
    local.get $p23
    call $rt_arr__len
    i64.extend_i32_s
    local.set $p24
    local.get $p24
    call $rt_str__from_i64
    local.set $p25
    local.get $p25
    call $rt_core__println
    i32.const 0
    local.set $p26
    local.get $p23
    local.set $p27
    local.get $p27
    call $rt_arr__len
    i64.extend_i32_s
    local.set $p28
    local.get $p28
    local.set $p29
    i64.const 0
    local.set $p30
    (block $break_1 (result i32)
      (loop $cont_1
        local.get $p30
        local.get $p29
        i64.ge_s
        local.set $p31
        local.get $p31
        (if
          (then
            i32.const 0
            br $break_1)
          (else
            local.get $p27
            local.get $p30
            i32.wrap_i64
            call $rt_arr__get
            ref.cast (ref i31)
            i31.get_s
            local.set $p32
            local.get $p32
            local.set $p33
            local.get $p30
            i64.const 1
            i64.add
            local.set $p34
            local.get $p34
            local.set $p30
            i32.const 0
            local.set $p35
            local.get $p33
            i64.extend_i32_u
            local.set $p36
            local.get $p36
            call $rt_str__from_i64
            local.set $p37
            local.get $p37
            call $rt_core__println
            i32.const 0
            local.set $p38
            br $cont_1))
        local.get $p39
        drop
        br $cont_1)
      unreachable)
    local.set $p40
    local.get $p23
    call $user__$string_from_utf8_helper
    local.set $p41
    local.get $p41
    ref.test (ref null $rt_types__Variant)
    (if (result (ref null $rt_types__Variant))
      (then
        local.get $p41
        ref.cast (ref null $rt_types__Variant))
      (else
        i32.const 0
        local.get $p41
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        local.get $p41
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        i32.const 1
        i32.eq
        (if (result (ref $rt_types__Array))
          (then
            local.get $p41
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 1
            array.new_fixed $rt_types__Array 1)
          (else
            array.new_fixed $rt_types__Array 0))
        struct.new $rt_types__Variant))
    call $user__func_41
    local.set $p42
    local.get $p42
    call $rt_core__println
    i32.const 0
    local.set $p43
    call $user____str_lit_get_f09f918d
    call $user__$string_utf8_bytes_helper
    local.set $p44
    local.get $p44
    ref.cast (ref null $rt_types__Array)
    local.set $p45
    local.get $p45
    call $rt_arr__len
    i64.extend_i32_s
    local.set $p46
    local.get $p46
    call $rt_str__from_i64
    local.set $p47
    local.get $p47
    call $rt_core__println
    i32.const 0
    local.set $p48
    local.get $p45
    local.set $p49
    local.get $p49
    call $rt_arr__len
    i64.extend_i32_s
    local.set $p50
    local.get $p50
    local.set $p51
    i64.const 0
    local.set $p52
    (block $break_2 (result i32)
      (loop $cont_2
        local.get $p52
        local.get $p51
        i64.ge_s
        local.set $p53
        local.get $p53
        (if
          (then
            i32.const 0
            br $break_2)
          (else
            local.get $p49
            local.get $p52
            i32.wrap_i64
            call $rt_arr__get
            ref.cast (ref i31)
            i31.get_s
            local.set $p54
            local.get $p54
            local.set $p55
            local.get $p52
            i64.const 1
            i64.add
            local.set $p56
            local.get $p56
            local.set $p52
            i32.const 0
            local.set $p57
            local.get $p55
            i64.extend_i32_u
            local.set $p58
            local.get $p58
            call $rt_str__from_i64
            local.set $p59
            local.get $p59
            call $rt_core__println
            i32.const 0
            local.set $p60
            br $cont_2))
        local.get $p61
        drop
        br $cont_2)
      unreachable)
    local.set $p62
    local.get $p45
    call $user__$string_from_utf8_helper
    local.set $p63
    local.get $p63
    ref.test (ref null $rt_types__Variant)
    (if (result (ref null $rt_types__Variant))
      (then
        local.get $p63
        ref.cast (ref null $rt_types__Variant))
      (else
        i32.const 0
        local.get $p63
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        local.get $p63
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        i32.const 1
        i32.eq
        (if (result (ref $rt_types__Array))
          (then
            local.get $p63
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 1
            array.new_fixed $rt_types__Array 1)
          (else
            array.new_fixed $rt_types__Array 0))
        struct.new $rt_types__Variant))
    call $user__func_41
    local.set $p64
    local.get $p64
    call $rt_core__println
    i32.const 0
    local.set $p65
    call $user____str_lit_get_empty
    call $user__$string_utf8_bytes_helper
    local.set $p66
    local.get $p66
    ref.cast (ref null $rt_types__Array)
    local.set $p67
    local.get $p67
    call $rt_arr__len
    i64.extend_i32_s
    local.set $p68
    local.get $p68
    call $rt_str__from_i64
    local.set $p69
    local.get $p69
    call $rt_core__println
    i32.const 0
    local.set $p70
    local.get $p67
    call $user__$string_from_utf8_helper
    local.set $p71
    local.get $p71
    ref.test (ref null $rt_types__Variant)
    (if (result (ref null $rt_types__Variant))
      (then
        local.get $p71
        ref.cast (ref null $rt_types__Variant))
      (else
        i32.const 0
        local.get $p71
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        local.get $p71
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        i32.const 1
        i32.eq
        (if (result (ref $rt_types__Array))
          (then
            local.get $p71
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 1
            array.new_fixed $rt_types__Array 1)
          (else
            array.new_fixed $rt_types__Array 0))
        struct.new $rt_types__Variant))
    call $user__func_41
    local.set $p72
    local.get $p72
    call $rt_core__println
    i32.const 0
    local.set $p73
    i64.const 128
    call $user__func_42
    local.set $p74
    local.get $p74
    ref.i31
    array.new_fixed $rt_types__Array 1
    local.set $p75
    local.get $p75
    local.set $p76
    local.get $p76
    call $user__$string_from_utf8_helper
    local.set $p77
    local.get $p77
    ref.test (ref null $rt_types__Variant)
    (if (result (ref null $rt_types__Variant))
      (then
        local.get $p77
        ref.cast (ref null $rt_types__Variant))
      (else
        i32.const 0
        local.get $p77
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        local.get $p77
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        i32.const 1
        i32.eq
        (if (result (ref $rt_types__Array))
          (then
            local.get $p77
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 1
            array.new_fixed $rt_types__Array 1)
          (else
            array.new_fixed $rt_types__Array 0))
        struct.new $rt_types__Variant))
    call $user__func_41
    local.set $p78
    local.get $p78
    call $rt_core__println
    i32.const 0
    local.set $p79
    i64.const 192
    call $user__func_42
    local.set $p80
    i64.const 128
    call $user__func_42
    local.set $p81
    local.get $p80
    ref.i31
    local.get $p81
    ref.i31
    array.new_fixed $rt_types__Array 2
    local.set $p82
    local.get $p82
    local.set $p83
    local.get $p83
    call $user__$string_from_utf8_helper
    local.set $p84
    local.get $p84
    ref.test (ref null $rt_types__Variant)
    (if (result (ref null $rt_types__Variant))
      (then
        local.get $p84
        ref.cast (ref null $rt_types__Variant))
      (else
        i32.const 0
        local.get $p84
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        local.get $p84
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        i32.const 1
        i32.eq
        (if (result (ref $rt_types__Array))
          (then
            local.get $p84
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 1
            array.new_fixed $rt_types__Array 1)
          (else
            array.new_fixed $rt_types__Array 0))
        struct.new $rt_types__Variant))
    call $user__func_41
    local.set $p85
    local.get $p85
    call $rt_core__println
    i32.const 0
    local.set $p86
    i64.const 224
    call $user__func_42
    local.set $p87
    local.get $p87
    ref.i31
    array.new_fixed $rt_types__Array 1
    local.set $p88
    local.get $p88
    local.set $p89
    local.get $p89
    call $user__$string_from_utf8_helper
    local.set $p90
    local.get $p90
    ref.test (ref null $rt_types__Variant)
    (if (result (ref null $rt_types__Variant))
      (then
        local.get $p90
        ref.cast (ref null $rt_types__Variant))
      (else
        i32.const 0
        local.get $p90
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        local.get $p90
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        i32.const 1
        i32.eq
        (if (result (ref $rt_types__Array))
          (then
            local.get $p90
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 1
            array.new_fixed $rt_types__Array 1)
          (else
            array.new_fixed $rt_types__Array 0))
        struct.new $rt_types__Variant))
    call $user__func_41
    local.set $p91
    local.get $p91
    call $rt_core__println
    i32.const 0
    local.set $p92
    i64.const 97
    call $user__func_42
    local.set $p93
    i64.const 98
    call $user__func_42
    local.set $p94
    i64.const 99
    call $user__func_42
    local.set $p95
    local.get $p93
    ref.i31
    local.get $p94
    ref.i31
    local.get $p95
    ref.i31
    array.new_fixed $rt_types__Array 3
    local.set $p96
    local.get $p96
    local.set $p97
    local.get $p97
    call $user__$string_from_utf8_helper
    local.set $p98
    local.get $p98
    ref.test (ref null $rt_types__Variant)
    (if (result (ref null $rt_types__Variant))
      (then
        local.get $p98
        ref.cast (ref null $rt_types__Variant))
      (else
        i32.const 0
        local.get $p98
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        local.get $p98
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        i32.const 1
        i32.eq
        (if (result (ref $rt_types__Array))
          (then
            local.get $p98
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 1
            array.new_fixed $rt_types__Array 1)
          (else
            array.new_fixed $rt_types__Array 0))
        struct.new $rt_types__Variant))
    call $user__func_41
    local.set $p99
    local.get $p99
    call $rt_core__println
    i32.const 0
    local.set $p100
    call $user____str_lit_get_68656c6c6f
    call $user__$string_utf8_bytes_helper
    local.set $p101
    local.get $p101
    ref.cast (ref null $rt_types__Array)
    call $user__$string_from_utf8_helper
    local.set $p102
    local.get $p102
    ref.test (ref null $rt_types__Variant)
    (if (result (ref null $rt_types__Variant))
      (then
        local.get $p102
        ref.cast (ref null $rt_types__Variant))
      (else
        i32.const 0
        local.get $p102
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        local.get $p102
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        i32.const 1
        i32.eq
        (if (result (ref $rt_types__Array))
          (then
            local.get $p102
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 1
            array.new_fixed $rt_types__Array 1)
          (else
            array.new_fixed $rt_types__Array 0))
        struct.new $rt_types__Variant))
    ref.cast (ref null $rt_types__Variant)
    struct.get $rt_types__Variant 0
    i32.const 0
    i32.eq
    local.get $p102
    ref.test (ref null $rt_types__Variant)
    (if (result (ref null $rt_types__Variant))
      (then
        local.get $p102
        ref.cast (ref null $rt_types__Variant))
      (else
        i32.const 0
        local.get $p102
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        local.get $p102
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        i32.const 1
        i32.eq
        (if (result (ref $rt_types__Array))
          (then
            local.get $p102
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 1
            array.new_fixed $rt_types__Array 1)
          (else
            array.new_fixed $rt_types__Array 0))
        struct.new $rt_types__Variant))
    ref.cast (ref null $rt_types__Variant)
    struct.get $rt_types__Variant 1
    i32.const 1
    i32.eq
    i32.and
    (if (result i32)
      (then
        local.get $p102
        ref.test (ref null $rt_types__Variant)
        (if (result (ref null $rt_types__Variant))
          (then
            local.get $p102
            ref.cast (ref null $rt_types__Variant))
          (else
            i32.const 0
            local.get $p102
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 0
            local.get $p102
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 0
            i32.const 1
            i32.eq
            (if (result (ref $rt_types__Array))
              (then
                local.get $p102
                ref.cast (ref null $user__option__String)
                struct.get $user__option__String 1
                array.new_fixed $rt_types__Array 1)
              (else
                array.new_fixed $rt_types__Array 0))
            struct.new $rt_types__Variant))
        ref.cast (ref null $rt_types__Variant)
        struct.get $rt_types__Variant 2
        i32.const 0
        array.get $rt_types__Array
        ref.cast (ref null $rt_types__String)
        local.set $p103
        local.get $p103
        local.set $p104
        call $user____str_lit_get_726f756e642d747269703a20
        local.get $p104
        call $rt_str__concat
        local.set $p105
        local.get $p105
        call $rt_core__println
        i32.const 0
        local.set $p106
        local.get $p106)
      (else
        local.get $p102
        ref.test (ref null $rt_types__Variant)
        (if (result (ref null $rt_types__Variant))
          (then
            local.get $p102
            ref.cast (ref null $rt_types__Variant))
          (else
            i32.const 0
            local.get $p102
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 0
            local.get $p102
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 0
            i32.const 1
            i32.eq
            (if (result (ref $rt_types__Array))
              (then
                local.get $p102
                ref.cast (ref null $user__option__String)
                struct.get $user__option__String 1
                array.new_fixed $rt_types__Array 1)
              (else
                array.new_fixed $rt_types__Array 0))
            struct.new $rt_types__Variant))
        ref.cast (ref null $rt_types__Variant)
        struct.get $rt_types__Variant 0
        i32.const 0
        i32.eq
        local.get $p102
        ref.test (ref null $rt_types__Variant)
        (if (result (ref null $rt_types__Variant))
          (then
            local.get $p102
            ref.cast (ref null $rt_types__Variant))
          (else
            i32.const 0
            local.get $p102
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 0
            local.get $p102
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 0
            i32.const 1
            i32.eq
            (if (result (ref $rt_types__Array))
              (then
                local.get $p102
                ref.cast (ref null $user__option__String)
                struct.get $user__option__String 1
                array.new_fixed $rt_types__Array 1)
              (else
                array.new_fixed $rt_types__Array 0))
            struct.new $rt_types__Variant))
        ref.cast (ref null $rt_types__Variant)
        struct.get $rt_types__Variant 1
        i32.const 0
        i32.eq
        i32.and
        (if (result i32)
          (then
            call $user____str_lit_get_4641494c
            call $rt_core__println
            i32.const 0
            local.set $p107
            local.get $p107)
          (else
            call $user____str_lit_get_6e6f6e2d65786861757374697665206d6174636820696e205f5f696e69745f5f202846756e6349642834332929
            call $rt_core__trap
            unreachable
            unreachable))))
    local.set $p108
    call $user____str_lit_get_e4bda0e5a5bde4b896e7958c
    call $user__$string_utf8_bytes_helper
    local.set $p109
    local.get $p109
    ref.cast (ref null $rt_types__Array)
    call $user__$string_from_utf8_helper
    local.set $p110
    local.get $p110
    ref.test (ref null $rt_types__Variant)
    (if (result (ref null $rt_types__Variant))
      (then
        local.get $p110
        ref.cast (ref null $rt_types__Variant))
      (else
        i32.const 0
        local.get $p110
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        local.get $p110
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        i32.const 1
        i32.eq
        (if (result (ref $rt_types__Array))
          (then
            local.get $p110
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 1
            array.new_fixed $rt_types__Array 1)
          (else
            array.new_fixed $rt_types__Array 0))
        struct.new $rt_types__Variant))
    ref.cast (ref null $rt_types__Variant)
    struct.get $rt_types__Variant 0
    i32.const 0
    i32.eq
    local.get $p110
    ref.test (ref null $rt_types__Variant)
    (if (result (ref null $rt_types__Variant))
      (then
        local.get $p110
        ref.cast (ref null $rt_types__Variant))
      (else
        i32.const 0
        local.get $p110
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        local.get $p110
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        i32.const 1
        i32.eq
        (if (result (ref $rt_types__Array))
          (then
            local.get $p110
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 1
            array.new_fixed $rt_types__Array 1)
          (else
            array.new_fixed $rt_types__Array 0))
        struct.new $rt_types__Variant))
    ref.cast (ref null $rt_types__Variant)
    struct.get $rt_types__Variant 1
    i32.const 1
    i32.eq
    i32.and
    (if (result i32)
      (then
        local.get $p110
        ref.test (ref null $rt_types__Variant)
        (if (result (ref null $rt_types__Variant))
          (then
            local.get $p110
            ref.cast (ref null $rt_types__Variant))
          (else
            i32.const 0
            local.get $p110
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 0
            local.get $p110
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 0
            i32.const 1
            i32.eq
            (if (result (ref $rt_types__Array))
              (then
                local.get $p110
                ref.cast (ref null $user__option__String)
                struct.get $user__option__String 1
                array.new_fixed $rt_types__Array 1)
              (else
                array.new_fixed $rt_types__Array 0))
            struct.new $rt_types__Variant))
        ref.cast (ref null $rt_types__Variant)
        struct.get $rt_types__Variant 2
        i32.const 0
        array.get $rt_types__Array
        ref.cast (ref null $rt_types__String)
        local.set $p111
        local.get $p111
        local.set $p112
        call $user____str_lit_get_726f756e642d747269703a20
        local.get $p112
        call $rt_str__concat
        local.set $p113
        local.get $p113
        call $rt_core__println
        i32.const 0
        local.set $p114
        local.get $p114)
      (else
        local.get $p110
        ref.test (ref null $rt_types__Variant)
        (if (result (ref null $rt_types__Variant))
          (then
            local.get $p110
            ref.cast (ref null $rt_types__Variant))
          (else
            i32.const 0
            local.get $p110
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 0
            local.get $p110
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 0
            i32.const 1
            i32.eq
            (if (result (ref $rt_types__Array))
              (then
                local.get $p110
                ref.cast (ref null $user__option__String)
                struct.get $user__option__String 1
                array.new_fixed $rt_types__Array 1)
              (else
                array.new_fixed $rt_types__Array 0))
            struct.new $rt_types__Variant))
        ref.cast (ref null $rt_types__Variant)
        struct.get $rt_types__Variant 0
        i32.const 0
        i32.eq
        local.get $p110
        ref.test (ref null $rt_types__Variant)
        (if (result (ref null $rt_types__Variant))
          (then
            local.get $p110
            ref.cast (ref null $rt_types__Variant))
          (else
            i32.const 0
            local.get $p110
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 0
            local.get $p110
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 0
            i32.const 1
            i32.eq
            (if (result (ref $rt_types__Array))
              (then
                local.get $p110
                ref.cast (ref null $user__option__String)
                struct.get $user__option__String 1
                array.new_fixed $rt_types__Array 1)
              (else
                array.new_fixed $rt_types__Array 0))
            struct.new $rt_types__Variant))
        ref.cast (ref null $rt_types__Variant)
        struct.get $rt_types__Variant 1
        i32.const 0
        i32.eq
        i32.and
        (if (result i32)
          (then
            call $user____str_lit_get_4641494c
            call $rt_core__println
            i32.const 0
            local.set $p115
            local.get $p115)
          (else
            call $user____str_lit_get_6e6f6e2d65786861757374697665206d6174636820696e205f5f696e69745f5f202846756e6349642834332929
            call $rt_core__trap
            unreachable
            unreachable))))
    local.set $p116
    local.get $p116
    drop
  )
  (func $user__func_41__closure (type $functype_29)
    (param $p0 anyref)
    (param $p1 anyref)
    (result anyref)
    local.get $p1
    ref.cast (ref null $rt_types__Array)
    i32.const 0
    array.get $rt_types__Array
    ref.cast (ref null $rt_types__Variant)
    call $user__func_41
  )
  (func $user__func_42__closure (type $functype_29)
    (param $p0 anyref)
    (param $p1 anyref)
    (result anyref)
    local.get $p1
    ref.cast (ref null $rt_types__Array)
    i32.const 0
    array.get $rt_types__Array
    ref.cast (ref $rt_types__BoxedInt)
    struct.get $rt_types__BoxedInt 0
    call $user__func_42
    ref.i31
  )
  (func $user__func_43__closure (type $functype_29)
    (param $p0 anyref)
    (param $p1 anyref)
    (result anyref)
    call $user__func_43
    i32.const 0
    ref.i31
  )
  (func $user__user____iterator_next (type $functype_30)
    (param $p0 anyref)
    (result (ref null $rt_types__Variant))
    (local $p1 (ref null $rt_types__Variant))
    (local $p2 i32)
    (local $p3 anyref)
    (local $p4 (ref null $rt_types__IterState))
    local.get $p0
    ref.cast (ref null $rt_types__IterState)
    local.set $p4
    local.get $p4
    struct.get $rt_types__IterState 1
    ref.cast (ref $rt_types__Closure)
    struct.get $rt_types__Closure 1
    local.get $p4
    struct.get $rt_types__IterState 0
    array.new_fixed $rt_types__Array 1
    local.get $p4
    struct.get $rt_types__IterState 1
    ref.cast (ref $rt_types__Closure)
    struct.get $rt_types__Closure 0
    call_ref $rt_types__ClosureFunc
    ref.cast (ref $rt_types__Variant)
    local.set $p1
    local.get $p1
    struct.get $rt_types__Variant 1
    local.set $p2
    local.get $p2
    i32.eqz
    (if (result (ref null $rt_types__Variant))
      (then
        i32.const 0
        i32.const 0
        array.new_fixed $rt_types__Array 0
        struct.new $rt_types__Variant)
      (else
        local.get $p1
        struct.get $rt_types__Variant 2
        local.set $p3
        local.get $p3
        ref.cast (ref null $rt_types__Array)
        i32.const 0
        array.get $rt_types__Array
        local.get $p3
        ref.cast (ref null $rt_types__Array)
        i32.const 1
        array.get $rt_types__Array
        local.get $p4
        struct.get $rt_types__IterState 1
        struct.new $rt_types__IterState
        struct.new $user__UserRecord_5
        local.set $p3
        i32.const 0
        i32.const 1
        local.get $p3
        array.new_fixed $rt_types__Array 1
        struct.new $rt_types__Variant))
    return
  )
  (func $user__$int_from_string_helper (type $functype_31)
    (param $p0 (ref null $rt_types__String))
    (result anyref)
    (local $p1 i64)
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 i64)
    (local $p5 i32)
    (local $p6 i32)
    i64.const 1
    local.set $p4
    i32.const 1
    local.set $p6
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p3
    local.get $p3
    i32.eqz
    (if
      (then
        i32.const 0
        local.set $p6)
      (else
        local.get $p0
        ref.as_non_null
        i32.const 0
        array.get_u $rt_types__String
        local.set $p5
        local.get $p5
        i32.const 45
        i32.eq
        (if
          (then
            i64.const -1
            local.set $p4
            i32.const 1
            local.set $p2
            local.get $p3
            i32.const 1
            i32.eq
            (if
              (then
                i32.const 0
                local.set $p6)))
          (else
            local.get $p5
            i32.const 43
            i32.eq
            (if
              (then
                i32.const 1
                local.set $p2
                local.get $p3
                i32.const 1
                i32.eq
                (if
                  (then
                    i32.const 0
                    local.set $p6)))
              (else
                i32.const 0
                local.set $p2))))
        local.get $p6
        (if
          (then
            (block $$done
              (loop $$digit_loop
                local.get $p2
                local.get $p3
                i32.ge_s
                br_if $$done
                local.get $p0
                ref.as_non_null
                local.get $p2
                array.get_u $rt_types__String
                local.set $p5
                local.get $p5
                i32.const 48
                i32.lt_s
                local.get $p5
                i32.const 57
                i32.gt_s
                i32.or
                (if
                  (then
                    i32.const 0
                    local.set $p6
                    br $$done))
                local.get $p1
                i64.const 10
                i64.mul
                local.get $p5
                i32.const 48
                i32.sub
                i64.extend_i32_u
                i64.add
                local.set $p1
                local.get $p2
                i32.const 1
                i32.add
                local.set $p2
                br $$digit_loop))))))
    local.get $p6
    (if (result anyref)
      (then
        i32.const 0
        i32.const 1
        local.get $p1
        local.get $p4
        i64.mul
        struct.new $rt_types__BoxedInt
        array.new_fixed $rt_types__Array 1
        struct.new $rt_types__Variant)
      (else
        i32.const 0
        i32.const 0
        array.new_fixed $rt_types__Array 0
        struct.new $rt_types__Variant))
  )
  (func $user__$from_code_point_helper (type $functype_32)
    (param $p0 i32)
    (result anyref)
    local.get $p0
    i32.const 0
    i32.lt_s
    (if (result anyref)
      (then
        i32.const 0
        i32.const 0
        array.new_fixed $rt_types__Array 0
        struct.new $rt_types__Variant)
      (else
        local.get $p0
        i32.const 128
        i32.lt_u
        (if (result anyref)
          (then
            i32.const 0
            i32.const 1
            local.get $p0
            array.new_fixed $rt_types__String 1
            array.new_fixed $rt_types__Array 1
            struct.new $rt_types__Variant)
          (else
            local.get $p0
            i32.const 2048
            i32.lt_u
            (if (result anyref)
              (then
                i32.const 0
                i32.const 1
                local.get $p0
                i32.const 6
                i32.shr_u
                i32.const 192
                i32.or
                local.get $p0
                i32.const 63
                i32.and
                i32.const 128
                i32.or
                array.new_fixed $rt_types__String 2
                array.new_fixed $rt_types__Array 1
                struct.new $rt_types__Variant)
              (else
                local.get $p0
                i32.const 55296
                i32.ge_u
                local.get $p0
                i32.const 57343
                i32.le_u
                i32.and
                (if (result anyref)
                  (then
                    i32.const 0
                    i32.const 0
                    array.new_fixed $rt_types__Array 0
                    struct.new $rt_types__Variant)
                  (else
                    local.get $p0
                    i32.const 65535
                    i32.le_u
                    (if (result anyref)
                      (then
                        i32.const 0
                        i32.const 1
                        local.get $p0
                        i32.const 12
                        i32.shr_u
                        i32.const 224
                        i32.or
                        local.get $p0
                        i32.const 6
                        i32.shr_u
                        i32.const 63
                        i32.and
                        i32.const 128
                        i32.or
                        local.get $p0
                        i32.const 63
                        i32.and
                        i32.const 128
                        i32.or
                        array.new_fixed $rt_types__String 3
                        array.new_fixed $rt_types__Array 1
                        struct.new $rt_types__Variant)
                      (else
                        local.get $p0
                        i32.const 1114111
                        i32.le_u
                        (if (result anyref)
                          (then
                            i32.const 0
                            i32.const 1
                            local.get $p0
                            i32.const 18
                            i32.shr_u
                            i32.const 240
                            i32.or
                            local.get $p0
                            i32.const 12
                            i32.shr_u
                            i32.const 63
                            i32.and
                            i32.const 128
                            i32.or
                            local.get $p0
                            i32.const 6
                            i32.shr_u
                            i32.const 63
                            i32.and
                            i32.const 128
                            i32.or
                            local.get $p0
                            i32.const 63
                            i32.and
                            i32.const 128
                            i32.or
                            array.new_fixed $rt_types__String 4
                            array.new_fixed $rt_types__Array 1
                            struct.new $rt_types__Variant)
                          (else
                            i32.const 0
                            i32.const 0
                            array.new_fixed $rt_types__Array 0
                            struct.new $rt_types__Variant))))))))))))
  )
  (func $user__$string_utf8_bytes_helper (type $functype_33)
    (param $p0 (ref null $rt_types__String))
    (result (ref $rt_types__Array))
    (local $p1 i32)
    (local $p2 i32)
    (local $p3 (ref $rt_types__Array))
    local.get $p0
    array.len
    local.set $p1
    local.get $p1
    array.new_default $rt_types__Array
    local.set $p3
    i32.const 0
    local.set $p2
    (block $$break
      (loop $$continue
        local.get $p2
        local.get $p1
        i32.ge_u
        br_if $$break
        local.get $p3
        local.get $p2
        local.get $p0
        local.get $p2
        array.get_u $rt_types__String
        ref.i31
        array.set $rt_types__Array
        local.get $p2
        i32.const 1
        i32.add
        local.set $p2
        br $$continue))
    local.get $p3
  )
  (func $user__$string_from_utf8_helper (type $functype_34)
    (param $p0 (ref null $rt_types__Array))
    (result anyref)
    (local $p1 i32)
    (local $p2 i32)
    (local $p3 (ref null $rt_types__String))
    (local $p4 i32)
    (local $p5 i32)
    (local $p6 i32)
    local.get $p0
    array.len
    local.set $p1
    i32.const 1
    local.set $p5
    i32.const 0
    local.set $p2
    (block $$vbreak
      (loop $$vcont
        local.get $p2
        local.get $p1
        i32.ge_u
        br_if $$vbreak
        local.get $p0
        local.get $p2
        array.get $rt_types__Array
        ref.cast (ref i31)
        i31.get_u
        local.set $p4
        local.get $p4
        i32.const 128
        i32.lt_u
        (if
          (then
            local.get $p2
            i32.const 1
            i32.add
            local.set $p2)
          (else
            local.get $p4
            i32.const 192
            i32.ge_u
            local.get $p4
            i32.const 223
            i32.le_u
            i32.and
            (if
              (then
                local.get $p4
                i32.const 194
                i32.lt_u
                (if
                  (then
                    i32.const 0
                    local.set $p5
                    br $$vbreak))
                local.get $p2
                i32.const 1
                i32.add
                local.get $p1
                i32.ge_u
                (if
                  (then
                    i32.const 0
                    local.set $p5
                    br $$vbreak))
                local.get $p0
                local.get $p2
                i32.const 1
                i32.add
                array.get $rt_types__Array
                ref.cast (ref i31)
                i31.get_u
                local.set $p6
                local.get $p6
                i32.const 192
                i32.and
                i32.const 128
                i32.ne
                (if
                  (then
                    i32.const 0
                    local.set $p5
                    br $$vbreak))
                local.get $p2
                i32.const 2
                i32.add
                local.set $p2)
              (else
                local.get $p4
                i32.const 224
                i32.ge_u
                local.get $p4
                i32.const 239
                i32.le_u
                i32.and
                (if
                  (then
                    local.get $p2
                    i32.const 2
                    i32.add
                    local.get $p1
                    i32.ge_u
                    (if
                      (then
                        i32.const 0
                        local.set $p5
                        br $$vbreak))
                    local.get $p0
                    local.get $p2
                    i32.const 1
                    i32.add
                    array.get $rt_types__Array
                    ref.cast (ref i31)
                    i31.get_u
                    i32.const 192
                    i32.and
                    i32.const 128
                    i32.ne
                    (if
                      (then
                        i32.const 0
                        local.set $p5
                        br $$vbreak))
                    local.get $p0
                    local.get $p2
                    i32.const 2
                    i32.add
                    array.get $rt_types__Array
                    ref.cast (ref i31)
                    i31.get_u
                    i32.const 192
                    i32.and
                    i32.const 128
                    i32.ne
                    (if
                      (then
                        i32.const 0
                        local.set $p5
                        br $$vbreak))
                    local.get $p4
                    i32.const 224
                    i32.eq
                    (if
                      (then
                        local.get $p0
                        local.get $p2
                        i32.const 1
                        i32.add
                        array.get $rt_types__Array
                        ref.cast (ref i31)
                        i31.get_u
                        i32.const 160
                        i32.lt_u
                        (if
                          (then
                            i32.const 0
                            local.set $p5
                            br $$vbreak))))
                    local.get $p4
                    i32.const 237
                    i32.eq
                    (if
                      (then
                        local.get $p0
                        local.get $p2
                        i32.const 1
                        i32.add
                        array.get $rt_types__Array
                        ref.cast (ref i31)
                        i31.get_u
                        i32.const 160
                        i32.ge_u
                        (if
                          (then
                            i32.const 0
                            local.set $p5
                            br $$vbreak))))
                    local.get $p2
                    i32.const 3
                    i32.add
                    local.set $p2)
                  (else
                    local.get $p4
                    i32.const 240
                    i32.ge_u
                    local.get $p4
                    i32.const 244
                    i32.le_u
                    i32.and
                    (if
                      (then
                        local.get $p2
                        i32.const 3
                        i32.add
                        local.get $p1
                        i32.ge_u
                        (if
                          (then
                            i32.const 0
                            local.set $p5
                            br $$vbreak))
                        local.get $p0
                        local.get $p2
                        i32.const 1
                        i32.add
                        array.get $rt_types__Array
                        ref.cast (ref i31)
                        i31.get_u
                        i32.const 192
                        i32.and
                        i32.const 128
                        i32.ne
                        (if
                          (then
                            i32.const 0
                            local.set $p5
                            br $$vbreak))
                        local.get $p0
                        local.get $p2
                        i32.const 2
                        i32.add
                        array.get $rt_types__Array
                        ref.cast (ref i31)
                        i31.get_u
                        i32.const 192
                        i32.and
                        i32.const 128
                        i32.ne
                        (if
                          (then
                            i32.const 0
                            local.set $p5
                            br $$vbreak))
                        local.get $p0
                        local.get $p2
                        i32.const 3
                        i32.add
                        array.get $rt_types__Array
                        ref.cast (ref i31)
                        i31.get_u
                        i32.const 192
                        i32.and
                        i32.const 128
                        i32.ne
                        (if
                          (then
                            i32.const 0
                            local.set $p5
                            br $$vbreak))
                        local.get $p4
                        i32.const 240
                        i32.eq
                        (if
                          (then
                            local.get $p0
                            local.get $p2
                            i32.const 1
                            i32.add
                            array.get $rt_types__Array
                            ref.cast (ref i31)
                            i31.get_u
                            i32.const 144
                            i32.lt_u
                            (if
                              (then
                                i32.const 0
                                local.set $p5
                                br $$vbreak))))
                        local.get $p4
                        i32.const 244
                        i32.eq
                        (if
                          (then
                            local.get $p0
                            local.get $p2
                            i32.const 1
                            i32.add
                            array.get $rt_types__Array
                            ref.cast (ref i31)
                            i31.get_u
                            i32.const 144
                            i32.ge_u
                            (if
                              (then
                                i32.const 0
                                local.set $p5
                                br $$vbreak))))
                        local.get $p2
                        i32.const 4
                        i32.add
                        local.set $p2)
                      (else
                        i32.const 0
                        local.set $p5
                        br $$vbreak))))))))
        br $$vcont))
    local.get $p5
    i32.eqz
    (if (result anyref)
      (then
        i32.const 0
        i32.const 0
        array.new_fixed $rt_types__Array 0
        struct.new $rt_types__Variant)
      (else
        local.get $p1
        array.new_default $rt_types__String
        local.set $p3
        i32.const 0
        local.set $p2
        (block $$cbreak
          (loop $$ccont
            local.get $p2
            local.get $p1
            i32.ge_u
            br_if $$cbreak
            local.get $p3
            local.get $p2
            local.get $p0
            local.get $p2
            array.get $rt_types__Array
            ref.cast (ref i31)
            i31.get_u
            array.set $rt_types__String
            local.get $p2
            i32.const 1
            i32.add
            local.set $p2
            br $$ccont))
        i32.const 0
        i32.const 1
        local.get $p3
        array.new_fixed $rt_types__Array 1
        struct.new $rt_types__Variant))
  )
  (func $user____user_init (type $functype_28)
    call $user__func_43
  )
  (func $user____str_lit_get_empty (type $functype_35)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_empty
    ref.is_null
    (if
      (then
        array.new_fixed $rt_types__String 0
        global.set $user____str_lit_global_empty))
    global.get $user____str_lit_global_empty
    ref.as_non_null
  )
  (func $user____str_lit_get_29 (type $functype_35)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_29
    ref.is_null
    (if
      (then
        i32.const 41
        array.new_fixed $rt_types__String 1
        global.set $user____str_lit_global_29))
    global.get $user____str_lit_global_29
    ref.as_non_null
  )
  (func $user____str_lit_get_4641494c (type $functype_35)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_4641494c
    ref.is_null
    (if
      (then
        i32.const 70
        i32.const 65
        i32.const 73
        i32.const 76
        array.new_fixed $rt_types__String 4
        global.set $user____str_lit_global_4641494c))
    global.get $user____str_lit_global_4641494c
    ref.as_non_null
  )
  (func $user____str_lit_get_4e6f6e65 (type $functype_35)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_4e6f6e65
    ref.is_null
    (if
      (then
        i32.const 78
        i32.const 111
        i32.const 110
        i32.const 101
        array.new_fixed $rt_types__String 4
        global.set $user____str_lit_global_4e6f6e65))
    global.get $user____str_lit_global_4e6f6e65
    ref.as_non_null
  )
  (func $user____str_lit_get_536f6d6528 (type $functype_35)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_536f6d6528
    ref.is_null
    (if
      (then
        i32.const 83
        i32.const 111
        i32.const 109
        i32.const 101
        i32.const 40
        array.new_fixed $rt_types__String 5
        global.set $user____str_lit_global_536f6d6528))
    global.get $user____str_lit_global_536f6d6528
    ref.as_non_null
  )
  (func $user____str_lit_get_6261642062797465 (type $functype_35)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_6261642062797465
    ref.is_null
    (if
      (then
        i32.const 98
        i32.const 97
        i32.const 100
        i32.const 32
        i32.const 98
        i32.const 121
        i32.const 116
        i32.const 101
        array.new_fixed $rt_types__String 8
        global.set $user____str_lit_global_6261642062797465))
    global.get $user____str_lit_global_6261642062797465
    ref.as_non_null
  )
  (func $user____str_lit_get_68656c6c6f (type $functype_35)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_68656c6c6f
    ref.is_null
    (if
      (then
        i32.const 104
        i32.const 101
        i32.const 108
        i32.const 108
        i32.const 111
        array.new_fixed $rt_types__String 5
        global.set $user____str_lit_global_68656c6c6f))
    global.get $user____str_lit_global_68656c6c6f
    ref.as_non_null
  )
  (func $user____str_lit_get_6e6f6e2d65786861757374697665206d6174636820696e205f5f696e69745f5f202846756e6349642834332929 (type $functype_35)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_6e6f6e2d65786861757374697665206d6174636820696e205f5f696e69745f5f202846756e6349642834332929
    ref.is_null
    (if
      (then
        i32.const 110
        i32.const 111
        i32.const 110
        i32.const 45
        i32.const 101
        i32.const 120
        i32.const 104
        i32.const 97
        i32.const 117
        i32.const 115
        i32.const 116
        i32.const 105
        i32.const 118
        i32.const 101
        i32.const 32
        i32.const 109
        i32.const 97
        i32.const 116
        i32.const 99
        i32.const 104
        i32.const 32
        i32.const 105
        i32.const 110
        i32.const 32
        i32.const 95
        i32.const 95
        i32.const 105
        i32.const 110
        i32.const 105
        i32.const 116
        i32.const 95
        i32.const 95
        i32.const 32
        i32.const 40
        i32.const 70
        i32.const 117
        i32.const 110
        i32.const 99
        i32.const 73
        i32.const 100
        i32.const 40
        i32.const 52
        i32.const 51
        i32.const 41
        i32.const 41
        array.new_fixed $rt_types__String 45
        global.set $user____str_lit_global_6e6f6e2d65786861757374697665206d6174636820696e205f5f696e69745f5f202846756e6349642834332929))
    global.get $user____str_lit_global_6e6f6e2d65786861757374697665206d6174636820696e205f5f696e69745f5f202846756e6349642834332929
    ref.as_non_null
  )
  (func $user____str_lit_get_6e6f6e2d65786861757374697665206d6174636820696e2062797465202846756e6349642834322929 (type $functype_35)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_6e6f6e2d65786861757374697665206d6174636820696e2062797465202846756e6349642834322929
    ref.is_null
    (if
      (then
        i32.const 110
        i32.const 111
        i32.const 110
        i32.const 45
        i32.const 101
        i32.const 120
        i32.const 104
        i32.const 97
        i32.const 117
        i32.const 115
        i32.const 116
        i32.const 105
        i32.const 118
        i32.const 101
        i32.const 32
        i32.const 109
        i32.const 97
        i32.const 116
        i32.const 99
        i32.const 104
        i32.const 32
        i32.const 105
        i32.const 110
        i32.const 32
        i32.const 98
        i32.const 121
        i32.const 116
        i32.const 101
        i32.const 32
        i32.const 40
        i32.const 70
        i32.const 117
        i32.const 110
        i32.const 99
        i32.const 73
        i32.const 100
        i32.const 40
        i32.const 52
        i32.const 50
        i32.const 41
        i32.const 41
        array.new_fixed $rt_types__String 41
        global.set $user____str_lit_global_6e6f6e2d65786861757374697665206d6174636820696e2062797465202846756e6349642834322929))
    global.get $user____str_lit_global_6e6f6e2d65786861757374697665206d6174636820696e2062797465202846756e6349642834322929
    ref.as_non_null
  )
  (func $user____str_lit_get_6e6f6e2d65786861757374697665206d6174636820696e2073686f775f6f7074202846756e6349642834312929 (type $functype_35)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_6e6f6e2d65786861757374697665206d6174636820696e2073686f775f6f7074202846756e6349642834312929
    ref.is_null
    (if
      (then
        i32.const 110
        i32.const 111
        i32.const 110
        i32.const 45
        i32.const 101
        i32.const 120
        i32.const 104
        i32.const 97
        i32.const 117
        i32.const 115
        i32.const 116
        i32.const 105
        i32.const 118
        i32.const 101
        i32.const 32
        i32.const 109
        i32.const 97
        i32.const 116
        i32.const 99
        i32.const 104
        i32.const 32
        i32.const 105
        i32.const 110
        i32.const 32
        i32.const 115
        i32.const 104
        i32.const 111
        i32.const 119
        i32.const 95
        i32.const 111
        i32.const 112
        i32.const 116
        i32.const 32
        i32.const 40
        i32.const 70
        i32.const 117
        i32.const 110
        i32.const 99
        i32.const 73
        i32.const 100
        i32.const 40
        i32.const 52
        i32.const 49
        i32.const 41
        i32.const 41
        array.new_fixed $rt_types__String 45
        global.set $user____str_lit_global_6e6f6e2d65786861757374697665206d6174636820696e2073686f775f6f7074202846756e6349642834312929))
    global.get $user____str_lit_global_6e6f6e2d65786861757374697665206d6174636820696e2073686f775f6f7074202846756e6349642834312929
    ref.as_non_null
  )
  (func $user____str_lit_get_726f756e642d747269703a20 (type $functype_35)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_726f756e642d747269703a20
    ref.is_null
    (if
      (then
        i32.const 114
        i32.const 111
        i32.const 117
        i32.const 110
        i32.const 100
        i32.const 45
        i32.const 116
        i32.const 114
        i32.const 105
        i32.const 112
        i32.const 58
        i32.const 32
        array.new_fixed $rt_types__String 12
        global.set $user____str_lit_global_726f756e642d747269703a20))
    global.get $user____str_lit_global_726f756e642d747269703a20
    ref.as_non_null
  )
  (func $user____str_lit_get_78 (type $functype_35)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_78
    ref.is_null
    (if
      (then
        i32.const 120
        array.new_fixed $rt_types__String 1
        global.set $user____str_lit_global_78))
    global.get $user____str_lit_global_78
    ref.as_non_null
  )
  (func $user____str_lit_get_c3a9 (type $functype_35)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_c3a9
    ref.is_null
    (if
      (then
        i32.const 195
        i32.const 169
        array.new_fixed $rt_types__String 2
        global.set $user____str_lit_global_c3a9))
    global.get $user____str_lit_global_c3a9
    ref.as_non_null
  )
  (func $user____str_lit_get_e4bda0e5a5bde4b896e7958c (type $functype_35)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_e4bda0e5a5bde4b896e7958c
    ref.is_null
    (if
      (then
        i32.const 228
        i32.const 189
        i32.const 160
        i32.const 229
        i32.const 165
        i32.const 189
        i32.const 228
        i32.const 184
        i32.const 150
        i32.const 231
        i32.const 149
        i32.const 140
        array.new_fixed $rt_types__String 12
        global.set $user____str_lit_global_e4bda0e5a5bde4b896e7958c))
    global.get $user____str_lit_global_e4bda0e5a5bde4b896e7958c
    ref.as_non_null
  )
  (func $user____str_lit_get_f09f918d (type $functype_35)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_f09f918d
    ref.is_null
    (if
      (then
        i32.const 240
        i32.const 159
        i32.const 145
        i32.const 141
        array.new_fixed $rt_types__String 4
        global.set $user____str_lit_global_f09f918d))
    global.get $user____str_lit_global_f09f918d
    ref.as_non_null
  )
  (func $__linked_init (type $functype_28)
    call $user____user_init
  )
  (export "rt_arr__make" (func $rt_arr__make))
  (export "rt_arr__get" (func $rt_arr__get))
  (export "rt_arr__set" (func $rt_arr__set))
  (export "rt_arr__len" (func $rt_arr__len))
  (export "rt_arr__concat" (func $rt_arr__concat))
  (export "rt_arr__slice" (func $rt_arr__slice))
  (export "rt_arr__builder_new" (func $rt_arr__builder_new))
  (export "rt_arr__builder_from" (func $rt_arr__builder_from))
  (export "rt_arr__builder_push" (func $rt_arr__builder_push))
  (export "rt_arr__builder_freeze" (func $rt_arr__builder_freeze))
  (export "rt_str__len" (func $rt_str__len))
  (export "rt_str__concat" (func $rt_str__concat))
  (export "rt_str__substring" (func $rt_str__substring))
  (export "rt_str__eq" (func $rt_str__eq))
  (export "rt_str__cmp" (func $rt_str__cmp))
  (export "rt_str__from_i64" (func $rt_str__from_i64))
  (export "rt_str__from_f64" (func $rt_str__from_f64))
  (export "rt_str__from_bool" (func $rt_str__from_bool))
  (export "rt_dict__make" (func $rt_dict__make))
  (export "rt_dict__len" (func $rt_dict__len))
  (export "rt_dict__keys" (func $rt_dict__keys))
  (export "rt_dict__has" (func $rt_dict__has))
  (export "rt_dict__get" (func $rt_dict__get))
  (export "rt_dict__get_option" (func $rt_dict__get_option))
  (export "rt_dict__set" (func $rt_dict__set))
  (export "rt_dict__remove" (func $rt_dict__remove))
  (export "rt_dict__set_in_place" (func $rt_dict__set_in_place))
  (export "rt_dict__remove_in_place" (func $rt_dict__remove_in_place))
  (export "rt_core__print" (func $rt_core__print))
  (export "rt_core__println" (func $rt_core__println))
  (export "rt_core__eprint" (func $rt_core__eprint))
  (export "rt_core__eprintln" (func $rt_core__eprintln))
  (export "rt_core__trap" (func $rt_core__trap))
  (export "rt_core__eq" (func $rt_core__eq))
  (start $__linked_init)
)