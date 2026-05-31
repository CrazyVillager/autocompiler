#include <stdio.h>
#include <omp.h>
int main(void){
  double t0 = omp_get_wtime();
  double s = 0;
  #pragma omp parallel for reduction(+:s)
  for (long i = 0; i < 200000000L; i++) s += 1.0;
  double t1 = omp_get_wtime();
  printf("sum=%.0f\n", s);
  printf("TIME=%.6f\n", t1 - t0);   // autorun が拾う計測値
  return 0;
}
