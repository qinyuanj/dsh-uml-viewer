package demo;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

class BoardTest {
    @Test
    void insideChecksBothBounds() {
        Board board = new Board(5);
        assertTrue(board.inside(0));
        assertFalse(board.inside(5));
        assertFalse(board.inside(-1));
    }

    @Test
    void arithmetic() {
        Board board = new Board(5);
        assertEquals(25, board.area());
        assertEquals(2, board.half());
        assertEquals(3, board.max(3, 1));
        assertEquals(3, board.max(1, 3));
        assertEquals(-3, board.negate(3));
        assertFalse(board.flip(true));
    }

    @Test
    void cells() {
        assertEquals(2, new Board.Cell().value(3));
    }
}
