#include <stdio.h>
#include <pthread.h>
void* w(void* a){ printf("thread\n"); return NULL; }
int main(void){ pthread_t t; pthread_create(&t,NULL,w,NULL); pthread_join(t,NULL); return 0; }
