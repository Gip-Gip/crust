
if( cond ) code;
if( cond ) { code }
else code;
else { code };

for( init ; cond ; var inc ) { code }
while( cond ) code;
while( cond ) { code }
do { code } while( cond ) ;

type identifier;
type *identifier;

type func(type arg, ...);
type func(type arg, ...) {}

func(arg, ...);

&x;
*x;

## These two are identical
type (*func_ptr)(type arg, ...) = func;
type (*func_ptr)(type arg, ...) = &func;

