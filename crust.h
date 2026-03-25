#ifndef CRUST_H
#define CRUST_H

typedef unsigned char u8;
typedef unsigned short u16;
typedef unsigned int u32;
typedef unsigned long u64;

#ifdef CRUST_PTR_U32
typedef u32 usize;
#else
typedef u64 usize;
#endif

#define Result(ty) union {struct {u8 is_ok; ty result;};struct {u8 _pad1; u8 error_code};}

#endif
