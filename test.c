#include <stdio.h>
#include "crust.h"

typedef Result(char*) ResultStrErr;

char* hello = "Hello, World!";

ResultStrErr foo(u8 num) {
    ResultStrErr result;
    if(num == 1) {
        result.result = (char*)hello;
        result.is_ok = true;
    } else {
        result.is_ok = false;
    }

    return result;
}

int main(void) {
    ResultStrErr result_1;
    ResultStrErr result_2;

    result_1 = foo(1);
    result_2 = foo(2);

    if(result_2.is_ok) {
        printf("No Good!");
    }

    if(result_1.is_ok) {
        printf("%s\n", result_1.result);
    }
}
