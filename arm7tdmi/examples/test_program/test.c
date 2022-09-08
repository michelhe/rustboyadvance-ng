volatile int breakpoint_count = 0;
volatile int breakpoint_on_me() {
    breakpoint_count++;
}

int main() {
    int x = 1337;
    for (;;) {
        x += 1;
        if (x == 7331) {
            breakpoint_on_me();
            x = 1337;
        }
    }
    return 0;
}

